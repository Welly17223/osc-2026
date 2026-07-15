#![no_std]
use core::alloc::{GlobalAlloc, Layout};
use core::ffi::c_void;
use core::fmt::Write;
use core::result::Result::Ok;
use core::{ffi, mem};

use oslib::interrupt::timer::{self};
use oslib::memory_alloc::ALLOCATOR;
use oslib::uart::SERIAL;

unsafe extern "C" {
    pub fn test_alloc_1();
    pub fn test_addtask();
    fn test_task_cb(args: *const ffi::c_void);
    // fn test_task_cb1(args: *const ffi::c_void);
    pub fn test_func(args: *const u8);
}

pub extern "C" fn test_task_cb_wrapper(args: *const u8) {
    unsafe {
        test_task_cb(args as *const ffi::c_void);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn allocate(size: ffi::c_ulong) -> *mut u8 {
    let leading = size.leading_zeros();
    let align = 1 << (usize::BITS - leading);

    unsafe { ALLOCATOR.alloc(Layout::from_size_align_unchecked(size as usize, align)) }
}

#[unsafe(no_mangle)]
/// # Safety
pub unsafe extern "C" fn free(ptr: *mut u8) {
    unsafe {
        ALLOCATOR.dealloc(ptr, Layout::new::<usize>());
    }
}

#[unsafe(no_mangle)]
/// # Safety
pub unsafe extern "C" fn uart_puts(s: *const ffi::c_char) {
    let serial = &raw mut SERIAL;
    let Some(serial) = (unsafe { &mut *serial }) else {
        return;
    };
    let Ok(s) = unsafe { ffi::CStr::from_ptr(s) }.to_str() else {
        return;
    };
    write!(serial, "{}", s).unwrap();
}

#[unsafe(no_mangle)]
/// # Safety
pub unsafe extern "C" fn uart_hex(s: ffi::c_long) {
    let serial = &raw mut SERIAL;
    let Some(serial) = (unsafe { &mut *serial }) else {
        return;
    };
    write!(serial, "{:#x}", s).unwrap();
}

#[unsafe(no_mangle)]
/// # Safety
pub unsafe extern "C" fn uart_putc(s: ffi::c_char) {
    let serial = &raw mut SERIAL;
    let Some(serial) = (unsafe { &mut *serial }) else {
        return;
    };
    write!(serial, "{}", s).unwrap();
}

#[unsafe(no_mangle)]
/// # Safety
pub unsafe extern "C" fn add_task(
    callback: unsafe extern "C" fn(*const ffi::c_void),
    args: *const ffi::c_void,
    priority: ffi::c_int,
) {
    let callback: extern "C" fn(*const u8) = unsafe { mem::transmute(callback) };
    oslib::interrupt::add_task_c(callback, args, priority);
}

#[unsafe(no_mangle)]
pub extern "C" fn add_timer(callback: *const c_void, args: *const c_void, delay: ffi::c_ulong) {
    if callback.is_null() {
        let callback = |_| {};
        timer::add_timer_c(delay, callback, args as *const u8, false);
    } else {
        let callback: fn(*const u8) = unsafe { mem::transmute(callback) };
        timer::add_timer_c(delay, callback, args as *const u8, false);
    }
}

pub fn test_func_wrapper(args: *const u8) {
    unsafe { test_func(args) };
}
