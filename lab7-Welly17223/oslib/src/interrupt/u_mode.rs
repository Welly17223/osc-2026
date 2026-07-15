extern crate alloc;
use super::pt_regs;
use crate::{
    display,
    file_system::{self, OpenFlags, SeekFrom, VnodeType},
    interrupt::{InterruptTrait, SetStatusSUM, timer},
    schedule::{self, current_pid, current_tcb},
    thread::{self},
    uart::{self, get_serial},
    virtual_mem::{self, vm_area::Provider},
};
use alloc::{boxed::Box, string::String, vec, vec::Vec};
use bitflags::bitflags;
use core::{
    arch::asm,
    ffi::{self, CStr},
    fmt::Write,
    ptr, slice,
};

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
                    let result = get_serial().pop_rx() as _;
                    let _set_sstatus_sum = SetStatusSUM::new();
                    *i = result;
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
                let _save = SetStatusSUM::new();
                let file_name =
                    match unsafe { ffi::CStr::from_ptr(regs.a0 as *const ffi::c_char) }.to_str() {
                        Ok(s) => s,
                        Err(_) => {
                            regs.a0 = -1_isize as usize;
                            return;
                        }
                    };
                let vfs = file_system::ROOT.get().unwrap();

                if let Ok(file) = vfs.open(file_name, file_system::OpenFlags::from("r")) {
                    let tcb = current_tcb();
                    let mut new_vm_mapper = virtual_mem::vm_area::Manager::new();
                    new_vm_mapper
                        .map_file_addr(0_usize.into(), file, virtual_mem::PROT_USER_TEXT)
                        .unwrap();

                    new_vm_mapper
                        .map_addr(
                            virtual_mem::VirtualAddress(
                                virtual_mem::USER_MODE_STACK_ADDRESS.addr()
                                    - 2 * virtual_mem::PMD_SIZE,
                            ),
                            2 * virtual_mem::PMD_SIZE,
                            virtual_mem::PROT_USER_STACK,
                            virtual_mem::vm_area::Provider::Anonymous,
                        )
                        .unwrap();

                    let mut sig = thread::SigAct::default();

                    let user_text = unsafe {
                        &*ptr::slice_from_raw_parts(
                            &thread::__user_text_start as *const usize as *const u8,
                            (&thread::__user_text_end) as *const usize as usize
                                - (&thread::__user_text_start) as *const usize as usize,
                        )
                    };

                    let sig_ret_func = thread::sig_ret as *const () as usize;
                    let sig_ret_func_offset = sig_ret_func & 0xfff;
                    let u_mode_do_exit_offset =
                        (thread::u_mode_do_exit as *const () as usize) & 0xfff;
                    let mut sig_ret_func_copied: Box<[u8]> = Box::from([0u8; 0x1000]);
                    sig_ret_func_copied[..user_text.len()].copy_from_slice(user_text);

                    let sig_map_base = new_vm_mapper
                        .map(
                            user_text.len(),
                            virtual_mem::PROT_USER_TEXT,
                            Provider::Mem(user_text),
                        )
                        .unwrap();

                    sig.sig_ret_addr = sig_map_base.addr() + sig_ret_func_offset;

                    tcb.context.satp = new_vm_mapper.satp();
                    tcb.vm_mapper = Some(Box::new(new_vm_mapper));
                    *tcb.sig = sig;

                    unsafe {
                        asm!("csrw satp, {}", in(reg) tcb.context.satp);
                        asm!("sfence.vma");
                    }

                    regs.sepc = virtual_mem::USER_MODE_START_ADDRESS.addr();
                    regs.sscratch = virtual_mem::USER_MODE_STACK_ADDRESS.addr();
                    regs.a0 = 0;
                    regs.ra = sig_map_base.addr() + u_mode_do_exit_offset;
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
                let save = SetStatusSUM::new();
                let prev_regs = unsafe { (regs.sscratch as *const pt_regs).read_volatile() };
                let curr_sig = &mut current_tcb().sig;
                let sig_stack = curr_sig.sig_stack.as_ref().unwrap();
                let sig_stack_buttom = sig_stack.addr();
                let sig_stack_top = sig_stack_buttom + thread::SIG_STACK_SIZE;
                drop(save);

                if !(prev_regs.sscratch > sig_stack_buttom && prev_regs.sscratch < sig_stack_top) {
                    let stack_top = curr_sig.sig_stack.take().unwrap();
                    current_tcb()
                        .vm_mapper
                        .as_mut()
                        .unwrap()
                        .unmap(stack_top)
                        .unwrap();
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
            SModeInterruptEnum::Mmap => {
                use virtual_mem::*;
                let tcb = current_tcb();

                let addr = regs.a0 as *const u8;
                let length = crate::align(regs.a1, 0x1000);
                let prop = ((regs.a2 & 0b1111) << 1) | PTE_U;
                let flags = MmapFlags::from_bits(regs.a3).unwrap();

                let mapper = tcb.vm_mapper.as_mut().unwrap();
                let start_addr = if addr.is_null() {
                    mapper.map_addr(
                        VirtualAddress(addr as usize),
                        length,
                        prop,
                        vm_area::Provider::Anonymous,
                    )
                } else {
                    mapper.map(length, prop, vm_area::Provider::Anonymous)
                }
                .unwrap();

                if flags.contains(MmapFlags::Populate) {
                    for i in (start_addr.addr()..(start_addr.addr() + length)).step_by(0x1000) {
                        let _ = mapper.map_to_phy(i.into());
                    }
                }

                let serial = get_serial();
                writeln!(
                    serial,
                    "mmap: expect at: {:#x} alloc at {:#x} size: {} prop: {:#b}, flags: {:#x}",
                    addr as usize, start_addr, length, prop, flags
                )
                .unwrap();

                regs.a0 = start_addr.addr();
            }
            SModeInterruptEnum::Open => {
                let tcb = current_tcb();
                let sstatus_sum = SetStatusSUM::new();
                let str = match unsafe { CStr::from_ptr(regs.a0 as *const u8) }.to_str() {
                    Ok(s) => String::from(s),
                    Err(_) => {
                        regs.a0 = -1_isize as usize;
                        return;
                    }
                };
                drop(sstatus_sum);
                let flags = regs.a1;
                regs.a0 = match tcb.fdt.open(
                    &str,
                    OpenFlags {
                        create: flags == file_system::O_CREAT,
                        read: true,
                        write: true,
                    },
                ) {
                    Ok(fd) => fd,
                    Err(_) => -1_isize as usize,
                };
            }
            SModeInterruptEnum::Close => {
                let tcb = current_tcb();
                let fd = regs.a0;

                regs.a0 = match tcb.fdt.close(fd) {
                    Ok(()) => 0,
                    Err(_) => -1_isize as usize,
                };
            }
            SModeInterruptEnum::Read => {
                let tcb = current_tcb();
                let fd = regs.a0;
                let len = regs.a2;
                let mut buf = vec![0u8; len];

                let Some(file) = &mut tcb.fdt[fd] else {
                    regs.a0 = -1_isize as usize;
                    return;
                };

                regs.a0 = match file.read(buf.as_mut()) {
                    Ok(size) => {
                        let _sstatus_sum = SetStatusSUM::new();
                        let user_buf = unsafe {
                            &mut *ptr::slice_from_raw_parts_mut(regs.a1 as *mut u8, regs.a2)
                        };

                        user_buf[..size].copy_from_slice(&buf[..size]);

                        size
                    }
                    Err(_) => -1_isize as usize,
                };
            }
            SModeInterruptEnum::Write => {
                let tcb = current_tcb();
                let fd = regs.a0;

                let Some(file) = &mut tcb.fdt[fd] else {
                    regs.a0 = -1_isize as usize;
                    return;
                };

                let sstatus_sum = SetStatusSUM::new();
                let buf = Vec::from(unsafe {
                    &*ptr::slice_from_raw_parts(regs.a1 as *const u8, regs.a2)
                });
                drop(sstatus_sum);

                regs.a0 = match file.write(&buf) {
                    Ok(size) => size,
                    Err(_) => -1_isize as usize,
                }
            }
            SModeInterruptEnum::Mkdir => {
                let sstatus_sum = SetStatusSUM::new();
                let path = match unsafe { CStr::from_ptr(regs.a0 as *const u8) }.to_str() {
                    Ok(s) => String::from(s),
                    Err(_) => {
                        regs.a0 = -1_isize as usize;
                        return;
                    }
                };
                drop(sstatus_sum);

                let vfs = file_system::ROOT.get().unwrap();
                regs.a0 = match vfs.mkdir(&path, false) {
                    Ok(_) => 0,
                    Err(_) => -1_isize as _,
                };
            }
            SModeInterruptEnum::Mount => {
                let sstatus_sum = SetStatusSUM::new();
                let path = match unsafe { CStr::from_ptr(regs.a1 as *const u8) }.to_str() {
                    Ok(s) => String::from(s),
                    Err(_) => {
                        regs.a0 = -1_isize as usize;
                        return;
                    }
                };
                let fs = match unsafe { CStr::from_ptr(regs.a2 as *const u8) }.to_str() {
                    Ok(s) => String::from(s),
                    Err(_) => {
                        regs.a0 = -1_isize as usize;
                        return;
                    }
                };
                drop(sstatus_sum);

                let vfs = file_system::ROOT.get().unwrap();
                regs.a0 = match vfs.mount(&path, &fs) {
                    Ok(_) => 0,
                    Err(_) => -1_isize as _,
                };
            }
            SModeInterruptEnum::Chdir => {
                let sstatus_sum = SetStatusSUM::new();
                let path = match unsafe { CStr::from_ptr(regs.a0 as *const u8) }.to_str() {
                    Ok(s) => String::from(s),
                    Err(_) => {
                        regs.a0 = -1_isize as usize;
                        return;
                    }
                };
                drop(sstatus_sum);

                let vfs = file_system::ROOT.get().unwrap();
                regs.a0 = match vfs.lookup(&path) {
                    Ok(node) if node.metadata.types == VnodeType::Directory => {
                        curr_thread.cwd = node;
                        0
                    }
                    Ok(_) | Err(_) => -1_isize as _,
                };
            }
            SModeInterruptEnum::LSeek64 => {
                let fd = regs.a0;
                let seek = SeekFrom::from_raw(regs.a1 as isize, regs.a2);

                let Some(file) = &mut current_tcb().fdt[fd] else {
                    regs.a0 = -1_isize as _;
                    return;
                };

                regs.a0 = match file.seek(seek) {
                    Ok(n) => n as _,
                    Err(_) => -1_isize as usize,
                };
            }
            SModeInterruptEnum::IOctl => {
                use virtual_mem::phy_to_virt;
                let fd = regs.a0;
                let req = regs.a1;
                let ptr = regs.a2;

                let tcb = current_tcb();
                let Some(file) = &mut tcb.fdt[fd] else {
                    regs.a0 = -1_isize as _;
                    return;
                };

                let curr_shift = virtual_mem::PTE_SHIFT;
                let pte_ptr = tcb
                    .vm_mapper
                    .as_ref()
                    .unwrap()
                    .page_entry_ref(ptr.into())
                    .unwrap();

                let offset = ptr - (ptr & !((1 << curr_shift) - 1));
                let kernel_ptr = phy_to_virt(pte_ptr.get_pa() + offset);

                regs.a0 = match file.ioctl(req, kernel_ptr.addr() as _) {
                    Ok(_) => {
                        let _sstatus = SetStatusSUM::new();
                        unsafe {
                            asm!(r#"
                                 cbo.inval ({usr})
                                 fence rw, rw
                                 fence.i
                                 "#,
                             usr = in(reg) ptr,
                            );
                        }
                        0
                    }
                    Err(_) => -1_isize as _,
                }
            }
        }
    }
}

pub enum SModeInterruptEnum {
    GetPID = 0,
    UartRead = 1,
    UartWrite = 2,
    Exec = 3,
    Fork = 4,
    WaitPID = 5,
    Exit = 6,
    Stop = 7,
    Display = 8,
    USleep = 9,
    Signal = 10,
    SigReturn = 11,
    Kill = 12,
    Mmap = 13,
    Open = 14,
    Close = 15,
    Read = 16,
    Write = 17,
    Mkdir = 18,
    Mount = 19,
    Chdir = 20,
    LSeek64 = 21,
    IOctl = 22,
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
            13 => Ok(Self::Mmap),
            14 => Ok(Self::Open),
            15 => Ok(Self::Close),
            16 => Ok(Self::Read),
            17 => Ok(Self::Write),
            18 => Ok(Self::Mkdir),
            19 => Ok(Self::Mount),
            20 => Ok(Self::Chdir),
            21 => Ok(Self::LSeek64),
            22 => Ok(Self::IOctl),
            _ => Err(()),
        }
    }
}

impl From<SModeInterruptEnum> for usize {
    fn from(value: SModeInterruptEnum) -> Self {
        value as _
    }
}

bitflags! {
    struct MmapFlags: usize {
        const Anonymous = 0x20;
        const Populate = 0x8000;
    }

    struct MmapProp: usize {
        const Read = 1;
        const Write = 1 << 1;
        const Exec = 1 << 2;
    }
}
