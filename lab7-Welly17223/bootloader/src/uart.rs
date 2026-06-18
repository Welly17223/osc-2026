#![allow(clippy::while_immutable_condition)]

use core::{cmp::max, fmt::Write, ptr};

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

pub struct Uart {
    base_addr: *const u8,
    register_shift: u32,
}

unsafe impl Send for Uart {}
unsafe impl Sync for Uart {}

impl Write for Uart {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.puts(s);
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
        }
    }

    gen_reg_methods!((Lsr, lsr), (Rbr, rbr), (Iir, iir));
    gen_reg_mut_methods!((Thr, thr), (Ier, ier), (Mcr, mcr));

    // This will set read and write buffer!
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
