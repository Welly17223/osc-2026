#![no_std]
use core::result::Result::Ok;
use core::alloc::{
    Layout,
    GlobalAlloc,
};
use core::ffi;

use memory_alloc::ALLOCATOR;
use log::info;

unsafe extern "C" {
    pub fn test_alloc_1();
}

#[unsafe(no_mangle)]
pub extern "C" fn allocate(size: ffi::c_ulong) -> *mut u8 {
    let leading = size.leading_zeros();
    let align = 1 << (usize::BITS - leading);

    unsafe { ALLOCATOR.alloc(Layout::from_size_align_unchecked(size as usize, align)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn free(ptr: *mut u8) {
    unsafe {
        ALLOCATOR.dealloc(ptr, Layout::new::<usize>());
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn uart_puts(s: *const ffi::c_char) {
    let Ok(s) = unsafe { ffi::CStr::from_ptr(s) }.to_str() else {
        return;
    };
    info!("{}", s);
}


