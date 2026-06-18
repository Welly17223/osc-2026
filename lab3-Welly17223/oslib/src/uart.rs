#![allow(clippy::while_immutable_condition)]

use crate::platform::*;
use core::fmt::Write;

pub const LSR_DR: u8 = 1 << 0;
pub const LSR_TDRQ: u8 = 1 << 5;

pub struct Uart {
    _base_addr: *const u8,
    listen_status_register: *const u8,
    receive_buffer_register: *const u8,
    transmit_holding_register: *mut u8,
}

unsafe impl Send for Uart {}

impl Write for Uart {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.puts(s);
        Ok(())
    }
}

impl Uart {
    pub fn new(base_addr: usize, listen_status_register: usize) -> Self {
        let listen_status_register = listen_status_register as *const u8;
        let base_addr = base_addr as *const u8;
        Uart {
            _base_addr: base_addr,
            listen_status_register,
            receive_buffer_register: base_addr.wrapping_byte_offset(0x0),
            transmit_holding_register: base_addr.wrapping_byte_offset(0x0) as *mut u8,
        }
    }

    pub fn getc(&self) -> u64 {
        while unsafe { self.listen_status_register.read_volatile() } & LSR_DR == 0 {}
        let ch: u64 = unsafe { self.receive_buffer_register.read_volatile() } as u64;
        if ch == '\r' as u64 { '\n' as u64 } else { ch }
    }

    pub fn get_raw_byte(&self) -> u64 {
        while unsafe { self.listen_status_register.read_volatile() } & LSR_DR == 0 {}
        let ch: u64 = unsafe { self.receive_buffer_register.read_volatile() } as u64;
        ch
    }

    pub fn get_u32(&self) -> u32 {
        let mut num = 0u32;
        for i in 0..4u32 {
            num |= (self.getc() as u32 & 0xff) << (i << 3);
        }
        num
    }

    pub fn putc(&self, ch: u8) {
        if ch == b'\n' {
            self.putc(b'\r');
        }
        while unsafe { self.listen_status_register.read_volatile() } & LSR_TDRQ == 0 {}
        unsafe { self.transmit_holding_register.write_volatile(ch) };
    }

    pub fn puts<T: AsRef<[u8]>>(&self, string: T) {
        for c in string.as_ref() {
            self.putc(*c);
        }
    }

    pub fn put_hex<T: Into<u64>>(&self, num: T) {
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

    pub fn put_dec(&self, mut num: u64) {
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
