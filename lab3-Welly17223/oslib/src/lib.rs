// #![cfg_attr(not(test), no_std)]
#![no_std]
#![no_main]

pub mod fdt;
pub mod logger;
pub mod platform;
pub mod ramdisk;
pub mod sbi;
pub mod uart;

pub fn add(left: u64, right: u64) -> u64 {
    left + right
}
