// #![cfg_attr(not(test), no_std)]
#![no_std]
#![no_main]

pub mod fdt;
pub mod interrupt;
pub mod logger;
pub mod platform;
pub mod ramdisk;
pub mod sbi;
pub mod schedule;
pub mod uart;

pub unsafe fn read_u64_from_ptr(ptr: *const u32) -> u64 {
    ((unsafe { *ptr } as u64) << 32) | unsafe { *ptr.wrapping_add(1) as u64 }
}

pub unsafe fn read_u64_from_ptr_swapbyte(ptr: *const u32) -> u64 {
    ((unsafe { *ptr }.swap_bytes() as u64) << 32)
        | unsafe { *ptr.wrapping_add(1) }.swap_bytes() as u64
}
