#![allow(clippy::while_immutable_condition)]

extern crate alloc;

use crate::interrupt::{self, plic};
use alloc::vec::Vec;
use core::{fmt::Write, ptr};

pub const LSR_DR: u8 = 1 << 0;
pub const LSR_TDRQ: u8 = 1 << 5;

enum Offset {
    Ier,
    Iir,
    Mcr,
    Lsr,
    Rbr,
    Thr,
}

impl From<Offset> for u8 {
    fn from(value: Offset) -> Self {
        match value {
            Offset::Rbr | Offset::Thr => 0x0,
            Offset::Ier => 0x1,
            Offset::Iir => 0x2,
            Offset::Mcr => 0x4,
            Offset::Lsr => 0x5,
        }
    }
}

impl From<Offset> for usize {
    fn from(value: Offset) -> Self {
        match value {
            Offset::Rbr | Offset::Thr => 0x0,
            Offset::Ier => 0x1,
            Offset::Iir => 0x2,
            Offset::Mcr => 0x4,
            Offset::Lsr => 0x5,
        }
    }
}

pub static mut SERIAL: Option<Uart> = None;

pub fn init_serial(dtb_addr: *const u8) {
    use crate::fdt;
    let offset_soc_serial = match fdt::path_offset(dtb_addr, "/soc/serial", 1) {
        Ok(o) => o,
        Err(_) => return,
    };
    let uart_shift = match fdt::getprop(dtb_addr, offset_soc_serial, "reg-shift") {
        Ok((ptr, _)) => unsafe { *(ptr as *const u32) }.swap_bytes(),
        Err(fdt::Error::Notfound) => 0,
        Err(_) => panic!("Error find reg-shift"),
    };
    let (reg_ptr, _) = match fdt::getprop(dtb_addr, offset_soc_serial, "reg") {
        Ok(prop) => (prop.0 as *mut u32, prop.1),
        Err(_) => return,
    };

    let uart_base = unsafe { *reg_ptr.wrapping_offset(1) }.swap_bytes() as usize;
    let uart_compatible = match fdt::getprop(dtb_addr, offset_soc_serial, "compatible") {
        Ok((ptr, size)) => {
            let u8_list = unsafe { &*ptr::slice_from_raw_parts(ptr, size) };
            unsafe { str::from_utf8_unchecked(u8_list) }
        }
        Err(_) => "",
    };
    let uart_irq = match fdt::getprop(dtb_addr, offset_soc_serial, "interrupts") {
        Ok((ptr, _len)) => unsafe { *(ptr as *const u32) }.swap_bytes(),
        Err(_) => todo!(),
    };
    let def_uart = match uart_compatible {
        // Qemu
        s if s.contains("ns16550a") || s.contains("pxa-uart") => Uart::new(uart_base, uart_shift),
        _ => unimplemented!(),
    };

    let Some(table) = &mut *crate::interrupt::IRQ_TABLE.write() else {
        return;
    };
    let _ = table.insert(uart_irq, crate::interrupt::plic::IRQ::UART);
    unsafe { SERIAL = Some(def_uart) };
}

const RING_BUF_SIZE: usize = 128;
struct TxQueue {
    // output_queue: [u8; TX_QUEUE_SIZE],
    queue: alloc::collections::LinkedList<Vec<u8>>,
    queue_idx: usize,
    // head: usize,
    // tail: usize,
    // size: usize,
}

struct RingBuf {
    output_queue: [u8; RING_BUF_SIZE],
    head: usize,
    tail: usize,
    size: usize,
}

impl Default for RingBuf {
    fn default() -> Self {
        Self {
            output_queue: [0u8; RING_BUF_SIZE],
            head: 0,
            tail: 0,
            size: 0,
        }
    }
}

impl RingBuf {
    fn push<T: AsRef<[u8]>>(&mut self, s: T) {
        s.as_ref().iter().for_each(|b| self.push_ch(*b));
    }

    fn push_ch<T: Into<u8>>(&mut self, ch: T) {
        let ch = ch.into();
        if self.size < self.output_queue.len() {
            self.output_queue[self.tail] = ch;
            self.tail = (self.tail + 1) & (self.output_queue.len() - 1);
            self.size += 1;
        }
    }

    fn pop(&mut self) -> Option<u8> {
        if self.size > 0 {
            let ch = self.output_queue[self.head];
            self.head = (self.head + 1) & (self.output_queue.len() - 1);
            self.size -= 1;
            Some(ch)
        } else {
            None
        }
    }

    fn is_empty(&self) -> bool {
        self.size == 0
    }
}

impl TxQueue {
    fn push(&mut self, s: &[u8]) {
        if !s.is_empty() {
            let mut bytes = Vec::with_capacity(s.len());
            s.iter().for_each(|b| {
                if *b == b'\n' {
                    bytes.push(b'\r');
                }
                bytes.push(*b);
            });

            self.queue.push_back(bytes);
        }
    }

    fn push_ch(&mut self, ch: u8) {
        if let Some(back) = self.queue.back_mut() {
            back.push(ch);
        } else {
            self.queue.push_back([ch].to_vec());
        }
    }

    fn pop(&mut self) -> Option<u8> {
        let front = self.queue.front()?;
        if self.queue_idx < front.len() {
            let res = Some(front[self.queue_idx]);
            self.queue_idx += 1;
            return res;
        }

        self.queue.pop_front();
        self.queue_idx = 0;
        let front = self.queue.front()?;
        let res = front[0];
        self.queue_idx += 1;
        Some(res)
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

pub struct Uart {
    base_addr: *const u8,
    register_shift: u32,
    rx_queue: Option<RingBuf>,
    tx_queue: Option<TxQueue>,
    is_async: bool,
}

unsafe impl Send for Uart {}
unsafe impl Sync for Uart {}

impl Write for Uart {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        if self.is_async() {
            let _disable_interrupt = interrupt::SModeInterrupt::new();
            self.push_tx(s.as_bytes());
        } else {
            self.puts(s);
        }
        Ok(())
    }
}

macro_rules! gen_reg_mut_methods {
    ($(($reg:ident, $func:ident)),* $(,)?) => {
        $(
            #[inline]
            fn $func(&mut self) -> *mut u8 {
                let offset: usize = Offset::$reg.into();
                self.base_addr.wrapping_byte_add(offset << self.register_shift) as *mut u8
            }
        )*
    };
}

macro_rules! gen_reg_methods {
    ($(( $reg:ident, $func:ident )),* $(,)?) => {
        $(
            #[inline]
            fn $func(&self) -> *const u8 {
                let offset: usize = Offset::$reg.into();
                self.base_addr.wrapping_byte_add(offset << self.register_shift)
            }
        )*
    };
}

impl Uart {
    pub fn new(base_addr: usize, register_shift: u32) -> Self {
        let base_addr = base_addr as *const u8;
        Uart {
            base_addr,
            register_shift,
            rx_queue: None,
            tx_queue: None,
            is_async: false,
        }
    }

    gen_reg_methods!((Lsr, lsr), (Rbr, rbr), (Iir, iir));
    gen_reg_mut_methods!((Thr, thr), (Ier, ier), (Mcr, mcr));

    // This will set read and write buffer!
    pub fn set_interrupt(&mut self, hart_id: usize) {
        use core::arch::asm;
        unsafe {
            let (mut ier, mut mcr) = if self.register_shift == 0 {
                (
                    self.ier().read_volatile() as u32,
                    self.mcr().read_volatile() as u32,
                )
            } else {
                (
                    (self.ier() as *const u32).read_volatile(),
                    (self.mcr() as *const u32).read_volatile(),
                )
            };

            ier |= 0b01;
            mcr |= 1 << 3;

            if self.register_shift == 0 {
                self.ier().write_volatile(ier as u8);
                self.mcr().write_volatile(mcr as u8);
            } else {
                (self.ier() as *mut u32).write_volatile(ier);
                (self.mcr() as *mut u32).write_volatile(mcr);
            }
        }

        let uart_irq = if let Some(irq_table) = &*interrupt::IRQ_TABLE.read() {
            let Some((res, _)) = irq_table.iter().find(|(_, irq)| **irq == plic::IRQ::UART) else {
                return;
            };
            *res
        } else {
            return;
        };
        writeln!(self, "uart_irq: {:#x}", uart_irq).unwrap();

        let hart0_plic = &raw const interrupt::plic::HART0_PLIC;
        if let Some(plic) = unsafe { &*hart0_plic } {
            writeln!(
                self,
                "set uart irq priority: [{}], {} bits at base: {:#x}",
                uart_irq >> 5,
                uart_irq & 0x1f,
                plic.get_base()
            )
            .unwrap();

            plic.set_priority(uart_irq, 0x2);
            plic.enable(hart_id, uart_irq as usize);
        }

        self.rx_queue = Some(RingBuf::default());
        self.tx_queue = Some(TxQueue {
            // output_queue: [0; TX_QUEUE_SIZE],
            queue: alloc::collections::LinkedList::new(),
            queue_idx: 0,
            // tail: 0,
            // head: 0,
            // size: 0,
        });

        self.is_async = true;
        unsafe {
            asm!("fence io, io", "fence.i");
        }
        self.puts("End interrupt setup");
    }

    pub fn is_async(&self) -> bool {
        self.is_async
    }

    pub fn push_tx(&mut self, s: &[u8]) {
        unsafe {
            let mut ier = self.ier().read_volatile();
            ier |= 0b10;
            self.ier().write_volatile(ier);
        };
        let Some(txq) = &mut self.tx_queue else {
            return;
        };
        txq.push(s);
    }

    pub fn push_tx_ch(&mut self, s: u8) {
        // set tx interrupt up
        unsafe {
            let mut ier = self.ier().read_volatile();
            ier |= 0b10;
            self.ier().write_volatile(ier);
        };
        let Some(txq) = &mut self.tx_queue else {
            return;
        };
        if s == b'\n' {
            txq.push_ch(b'\r');
        }
        txq.push_ch(s);
    }

    pub fn tx_interrupt(&mut self) {
        if let Some(ch) = self.pop_tx() {
            self.write_thr(ch);
        }
    }

    pub fn pop_tx(&mut self) -> Option<u8> {
        let Some(txq) = &mut self.tx_queue else {
            return None;
        };
        let res = txq.pop();
        if txq.is_empty() {
            unsafe { *self.ier() &= !0b10 };
        }
        res
    }

    pub fn push_rx(&mut self, c: u8) {
        let Some(rxq) = &mut self.rx_queue else {
            return;
        };
        rxq.push_ch(c);
    }

    pub fn get_iir(&self) -> u8 {
        unsafe { *self.iir() }
    }

    pub fn pop_rx_ch(&mut self) -> Option<u8> {
        if let Some(rxq) = &mut self.rx_queue {
            rxq.pop()
        } else {
            None
        }
    }

    pub fn rx_queue_empty(&self) -> bool {
        let Some(rxq) = &self.rx_queue else {
            return true;
        };

        rxq.is_empty()
    }

    pub fn write_thr(&mut self, ch: u8) {
        unsafe { self.thr().write_volatile(ch) };
    }

    pub fn read_lsr(&self) -> u64 {
        unsafe { self.rbr().read_volatile() as u64 }
    }

    pub fn getc(&self) -> u64 {
        while unsafe { self.lsr().read_volatile() } & LSR_DR == 0 {}
        let ch: u64 = unsafe { self.rbr().read_volatile() } as u64;
        if ch == '\r' as u64 { '\n' as u64 } else { ch }
    }

    pub fn get_raw_byte(&self) -> u64 {
        while unsafe { self.lsr().read_volatile() } & LSR_DR == 0 {}
        let ch: u64 = unsafe { self.rbr().read_volatile() } as u64;
        ch
    }

    pub fn get_u32(&self) -> u32 {
        let mut num = 0u32;
        for i in 0..4u32 {
            num |= (self.getc() as u32 & 0xff) << (i << 3);
        }
        num
    }

    pub fn putc(&mut self, ch: u8) {
        if ch == b'\n' {
            self.putc(b'\r');
        }
        while unsafe { self.lsr().read_volatile() } & LSR_TDRQ == 0 {}
        unsafe { self.thr().write_volatile(ch) };
    }

    pub fn puts<T: AsRef<[u8]>>(&mut self, string: T) {
        for c in string.as_ref() {
            self.putc(*c);
        }
    }

    pub fn put_hex<T: Into<u64>>(&mut self, num: T) {
        self.puts("0x");
        let mut n: u64;
        let num_size = (size_of::<T>() << 1) - 1;
        let num = num.into();
        for c in { 0..=num_size }.rev() {
            let c1 = c << 2;
            n = (num >> c1) & 0xf;
            n += if n > 9 { 0x57 } else { b'0' as u64 };
            self.putc(n as u8);
        }
    }

    pub fn put_dec(&mut self, mut num: u64) {
        let mut num_str = [0u8; 64];
        let mut num_len = 0usize;
        if num == 0 {
            self.putc(b'0');
            return;
        }

        while num > 0 && num_len < num_str.len() {
            num_str[num_len] = b'0' + (num % 10) as u8;
            num /= 10;
            num_len += 1;
        }

        let num_str_slice = &mut num_str[..num_len];
        num_str_slice.reverse();
        self.puts(num_str_slice);
    }
}

/*
pub fn getc() -> u64 {
    while unsafe { UART_LSR.read_volatile() } & LSR_DR == 0 {}
    let ch: u64 = unsafe { UART_RBR.read_volatile() } as u64;
    if ch == '\r' as u64 { '\n' as u64 } else { ch }
}

pub fn putc(ch: u8) {
    if ch == b'\n' {
        putc(b'\r');
    }
    while unsafe { UART_LSR.read_volatile() } & LSR_TDRQ == 0 {}
    unsafe { UART_THR.write_volatile(ch) };
}

pub fn puts<T: AsRef<[u8]>>(string: T) {
    for c in string.as_ref() {
        putc(*c);
    }
}

pub fn put_hex<T: Into<u64>>(num: T) {
    puts("0x");
    let num = num.into();
    let mut n: u64;
    for c in { 0..=15u64 }.rev() {
        let c1 = c << 2;
        n = (num >> c1) & 0xf;
        n += if n > 9 { 0x57 } else { b'0' as u64 };
        putc(n as u8);
    }
}
*/
