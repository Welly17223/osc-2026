#![allow(clippy::while_immutable_condition)]

use crate::platform::*;
pub const LSR_DR: u8 = 1 << 0;
pub const LSR_TDRQ: u8 = 1 << 5;

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

pub fn puts(string: &str) {
    for c in string.bytes() {
        putc(c);
    }
}

pub fn put_hex(num: u64) {
    puts("0x");
    let mut n: u64;
    for c in { 0..=15u64 }.rev() {
        let c1 = c << 2;
        n = (num >> c1) & 0xf;
        n += if n > 9 { 0x57 } else { b'0' as u64 };
        putc(n as u8);
    }
}
