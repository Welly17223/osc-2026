extern crate alloc;
use alloc::{boxed::Box, collections::LinkedList};
use crate::interrupt;
use core::arch::{self, asm};

#[repr(C)]
pub struct ThreadControlTable {
    context: Context,
    state: State,
    pid: usize,
}

#[derive(Default)]
#[repr(C)]
pub struct Context {
    pub ra: usize,
    pub sp: usize,
    pub s: [usize; 12],
}

pub struct Process {}

pub enum State {
    Ready,
    Waiting,
    Running,
    New,
    Terminate,
}

/// context switch
/// # Safety
///
/// This function only use in schedule function to execute context switch
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn switch_to(prev: *mut Context, next: *mut Context) {
    arch::naked_asm!(
        "sd ra , 8 *   0(a0)",
        "sd sp , 8 *   1(a0)",
        "sd s0 , 8 *   2(a0)",
        "sd s1 , 8 *   3(a0)",
        "sd s2 , 8 *   4(a0)",
        "sd s3 , 8 *   5(a0)",
        "sd s4 , 8 *   6(a0)",
        "sd s5 , 8 *   7(a0)",
        "sd s6 , 8 *   8(a0)",
        "sd s7 , 8 *   9(a0)",
        "sd s8 , 8 *  10(a0)",
        "sd s9 , 8 *  11(a0)",
        "sd s10, 8 *  12(a0)",
        "sd s11, 8 *  13(a0)",
        "ld ra , 8 *   0(a1)",
        "ld sp , 8 *   1(a1)",
        "ld s0 , 8 *   2(a1)",
        "ld s1 , 8 *   3(a1)",
        "ld s2 , 8 *   4(a1)",
        "ld s3 , 8 *   5(a1)",
        "ld s4 , 8 *   6(a1)",
        "ld s5 , 8 *   7(a1)",
        "ld s6 , 8 *   8(a1)",
        "ld s7 , 8 *   9(a1)",
        "ld s8 , 8 *  10(a1)",
        "ld s9 , 8 *  11(a1)",
        "ld s10, 8 *  12(a1)",
        "ld s11, 8 *  13(a1)",
        "mv tp, a1",
        "ret"
    );
}

static mut PROCESS_READY_QUEUE: Option<LinkedList<Box<ThreadControlTable>>> = None;
static mut CURR_PROCESS: Option<Box<ThreadControlTable>> = None;
static mut ALLOC_PID: usize = 0;

pub fn current_tcb() -> &'static mut ThreadControlTable {
    let curr_proc_ptr: *mut ThreadControlTable;
    unsafe {
        asm!("mv {}, tp", out(reg)curr_proc_ptr);
        &mut *curr_proc_ptr
    }
}

#[derive(Default)]
#[repr(C)]
pub struct RiscVRegs {
    pub ra: usize,
    pub sp: usize,
    pub gp: usize,
    pub tp: usize,
    pub t0: usize,
    pub t1: usize,
    pub t2: usize,
    pub s0: usize,
    pub s1: usize,
    pub a0: usize,
    pub a1: usize,
    pub a2: usize,
    pub a3: usize,
    pub a4: usize,
    pub a5: usize,
    pub a6: usize,
    pub a7: usize,
    pub s2: usize,
    pub s3: usize,
    pub s4: usize,
    pub s5: usize,
    pub s6: usize,
    pub s7: usize,
    pub s8: usize,
    pub s9: usize,
    pub s10: usize,
    pub s11: usize,
    pub t3: usize,
    pub t4: usize,
    pub t5: usize,
    pub t6: usize,
    pub sscratch: usize,
    pub sepc: usize,
    pub sstatus: usize,
}

pub fn schedule() {

    let thread_queue_ptr = &raw mut PROCESS_READY_QUEUE;
    let thread_queue = unsafe { &mut *thread_queue_ptr }.as_mut().unwrap();
    let curr_process = unsafe { &mut *(&raw mut CURR_PROCESS) }.take().unwrap();
    let curr_process_ptr = curr_process.as_ref() as *const _;

    if thread_queue.is_empty() {
        return;
    }

    thread_queue.push_back(curr_process);
    let next_proc = thread_queue.pop_front().unwrap();
    let next_proc_ptr = next_proc.as_ref() as *const _;
    unsafe { CURR_PROCESS = Some(next_proc) };

    unsafe {
        switch_to(curr_process_ptr as *mut _, next_proc_ptr as *mut _);
    }
}
