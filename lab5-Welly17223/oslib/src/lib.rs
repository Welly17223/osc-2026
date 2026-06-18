#![no_std]
#![no_main]


pub mod fdt;
pub mod interrupt;
pub mod logger;
pub mod platform;
pub mod ramdisk;
pub mod sbi;
pub mod schedule;
pub mod spinlock;
pub mod thread;
pub mod uart;
pub mod kernel_shell;
pub mod display;

pub unsafe fn read_u64_from_ptr(ptr: *const u32) -> u64 {
    ((unsafe { *ptr } as u64) << 32) | unsafe { *ptr.wrapping_add(1) as u64 }
}

pub unsafe fn read_u64_from_ptr_swapbyte(ptr: *const u32) -> u64 {
    ((unsafe { *ptr }.swap_bytes() as u64) << 32)
        | unsafe { *ptr.wrapping_add(1) }.swap_bytes() as u64
}

pub fn align(num: usize, aligned_number: usize) -> usize {
    (num + (aligned_number - 1)) & !(aligned_number - 1)
}
