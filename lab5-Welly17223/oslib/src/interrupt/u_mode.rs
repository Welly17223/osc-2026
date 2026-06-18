extern crate alloc;
use super::pt_regs;
use crate::{
    display,
    interrupt::{InterruptTrait, timer},
    ramdisk,
    schedule::{self, current_pid, current_tcb},
    thread::{self},
    uart::{self, get_serial},
};
use alloc::boxed::Box;
use core::{ffi, fmt::Write, ptr, slice};

pub struct Interrupt;

impl InterruptTrait for Interrupt {
    type InterruptEnum = SModeInterruptEnum;
    fn handler(regs: &mut pt_regs, interrupt_state: Option<Self::InterruptEnum>) {
        let curr_thread = current_tcb();

        let interrupt_state = match interrupt_state {
            Some(s) => s,
            None => {
                regs.a0 = usize::MAX;
                return;
            }
        };

        match interrupt_state {
            SModeInterruptEnum::GetPID => {
                regs.a0 = curr_thread.pid as _;
            }
            SModeInterruptEnum::UartRead => {
                let buf = unsafe { slice::from_raw_parts_mut(regs.a0 as *mut u8, regs.a1) };
                for i in buf.iter_mut() {
                    *i = get_serial().pop_rx() as _;
                }
                regs.a0 = regs.a1;
            }
            SModeInterruptEnum::UartWrite => {
                let serial = get_serial();
                let str = unsafe { &*ptr::slice_from_raw_parts(regs.a0 as *const u8, regs.a1) };
                serial.push_tx(str);
                regs.a0 = str.len();
            }
            SModeInterruptEnum::Exec => {
                let file_name =
                    unsafe { ffi::CStr::from_ptr(regs.a0 as *const ffi::c_char) }.to_bytes();
                if let Ok(file) = ramdisk::find(unsafe { ramdisk::INITRD_START as _ }, file_name) {
                    let tcb = current_tcb();
                    let new_start = tcb.stack.as_mut().unwrap();

                    new_start[0..file.len()].copy_from_slice(file);
                    regs.sepc = new_start.as_ptr() as _;
                    regs.sscratch = new_start
                        .as_ptr()
                        .wrapping_add(tcb.stack.as_ref().unwrap().len())
                        as _;
                    regs.a0 = 0;
                } else {
                    regs.a0 = -1_isize as _;
                }
            }
            SModeInterruptEnum::Fork => {
                schedule::USER_THREAD_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                regs.a0 = thread::ThreadControlTable::fork(current_tcb(), regs) as _;
            }
            SModeInterruptEnum::WaitPID => {
                let pid = regs.a0 as _;
                let lock = schedule::get_waitpid_queue_mut().lock();
                lock.get_mut().push_current(pid);
                drop(lock);
                writeln!(uart::get_serial(), "{} wait for {}", current_pid(), pid).unwrap();

                schedule::schedule();

                let arc = schedule::curr_thread_arc();
                let lock = arc.lock();
                lock.get_mut().children.remove(&pid);

                let term_queue = &mut lock.get_mut().term_children;
                let children = term_queue.remove(&pid).unwrap();
                regs.a0 = children.lock().get().exit_code as _;
            }
            SModeInterruptEnum::Exit => {
                let exit_code = regs.a0 as _;
                schedule::USER_THREAD_COUNT.fetch_sub(1, core::sync::atomic::Ordering::Relaxed);
                thread::do_exit(exit_code);
            }
            SModeInterruptEnum::Stop => {
                let target_pid = regs.a0 as _;
                let lock = schedule::get_live_proc().lock();
                let live_thread = lock.get_mut();

                regs.a0 = if let Some(target) = live_thread.get(&target_pid) {
                    target.lock().get_mut().state = thread::State::Terminate;
                    0
                } else {
                    -1_isize as _
                };
            }
            SModeInterruptEnum::Display => {
                display::video_bmp_display(regs.a0 as _, regs.a1, regs.a2);
            }
            SModeInterruptEnum::USleep => {
                let delay_usec = regs.a0;

                let current_process = schedule::curr_thread_arc();
                let lock = current_process.lock();
                let curr_proc = lock.get_mut();

                if curr_proc.state == thread::State::Running {
                    curr_proc.state = thread::State::Waiting;

                    let wait_usec: fn(*const u8) = |t: *const u8| {
                        let proc_box: Box<schedule::SafeSendTCB> =
                            unsafe { Box::from_raw(t as *mut _) };
                        let lock = proc_box.lock();
                        let proc = lock.get_mut();

                        if proc.state == thread::State::Waiting {
                            proc.state = thread::State::Ready;
                        }

                        drop(lock);
                        schedule::get_process_ready_queue_mut().push_back(*proc_box);
                    };

                    drop(lock);
                    let args = Box::new(current_process);

                    timer::add_timer(
                        timer::Time::new(delay_usec as _, timer::TimeUnit::MicroSec),
                        wait_usec,
                        Some(args),
                        false,
                    );
                }

                schedule::schedule();
                regs.a0 = 0;
            }
            SModeInterruptEnum::Signal => {
                let sig = regs.a0;
                let sig_handle = regs.a1;

                if sig >= u64::BITS as _ {
                    regs.a0 = 0;
                } else {
                    let curr_tcb = schedule::current_tcb();
                    let prev_signal = curr_tcb.sig.sig_handler_func[sig];
                    curr_tcb.sig.sig_handler_func[sig] = sig_handle;
                    regs.a0 = prev_signal;
                }
            }
            SModeInterruptEnum::SigReturn => {
                let prev_regs = unsafe { (regs.sscratch as *const pt_regs).read_volatile() };
                let curr_sig = &mut current_tcb().sig;
                let sig_stack = curr_sig.sig_stack.as_ref().unwrap();
                let sig_stack_buttom = sig_stack.as_ptr() as usize;
                let sig_stack_top = sig_stack_buttom + sig_stack.len();

                if !(prev_regs.sscratch > sig_stack_buttom && prev_regs.sscratch < sig_stack_top) {
                    curr_sig.sig_stack = None;
                }

                *regs = prev_regs;
            }
            SModeInterruptEnum::Kill => {
                let target_pid = regs.a0 as _;
                let target_sig = regs.a1;

                regs.a0 = if target_sig >= u64::BITS as _ {
                    -1_isize as _
                } else {
                    let live_proc_lock = schedule::get_live_proc().lock();
                    let live_proc_map = live_proc_lock.get_mut();

                    match live_proc_map.get_mut(&target_pid) {
                        Some(t) => {
                            let lock = t.lock();
                            let proc = lock.get_mut();
                            proc.sig.sig_mask |= 1 << target_sig;
                            0
                        }
                        None => -1_isize as _,
                    }
                }
            }
        }
    }
}

pub enum SModeInterruptEnum {
    GetPID,
    UartRead,
    UartWrite,
    Exec,
    Fork,
    WaitPID,
    Exit,
    Stop,
    Display,
    USleep,
    Signal,
    SigReturn,
    Kill,
}

impl TryFrom<usize> for SModeInterruptEnum {
    type Error = ();
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::GetPID),
            1 => Ok(Self::UartRead),
            2 => Ok(Self::UartWrite),
            3 => Ok(Self::Exec),
            4 => Ok(Self::Fork),
            5 => Ok(Self::WaitPID),
            6 => Ok(Self::Exit),
            7 => Ok(Self::Stop),
            8 => Ok(Self::Display),
            9 => Ok(Self::USleep),
            10 => Ok(Self::Signal),
            11 => Ok(Self::SigReturn),
            12 => Ok(Self::Kill),
            _ => Err(()),
        }
    }
}

impl From<SModeInterruptEnum> for usize {
    fn from(value: SModeInterruptEnum) -> Self {
        match value {
            SModeInterruptEnum::GetPID => 0,
            SModeInterruptEnum::UartRead => 1,
            SModeInterruptEnum::UartWrite => 2,
            SModeInterruptEnum::Exec => 3,
            SModeInterruptEnum::Fork => 4,
            SModeInterruptEnum::WaitPID => 5,
            SModeInterruptEnum::Exit => 6,
            SModeInterruptEnum::Stop => 7,
            SModeInterruptEnum::Display => 8,
            SModeInterruptEnum::USleep => 9,
            SModeInterruptEnum::Signal => 10,
            SModeInterruptEnum::SigReturn => 11,
            SModeInterruptEnum::Kill => 12,
        }
    }
}
