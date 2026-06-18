extern crate alloc;
use super::pt_regs;
use crate::{
    display,
    file_system::{self, OpenFlags, SeekFrom, VnodeType},
    interrupt::{InterruptTrait, SetStatusSUM, timer},
    ramdisk,
    schedule::{self, current_pid, current_tcb},
    thread::{self},
    uart::{self, get_serial},
    virtual_mem::{self, vpn0},
};
use alloc::{boxed::Box, string::String, vec, vec::Vec};
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
                let sstatus_sum = SetStatusSUM::new();
                let str =
                    unsafe { &*ptr::slice_from_raw_parts(regs.a0 as *const u8, regs.a1) }.to_vec();
                drop(sstatus_sum);
                serial.push_tx(&str);
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
                    let prev_file_len = tcb.text_file.take().unwrap().len().unwrap() as usize;
                    let curr_file_len = file.len().unwrap() as usize;
                    let root_pgd = current_tcb().pgd.as_mut().unwrap().as_mut().get_mut();

                    for addr in (0..prev_file_len).step_by(virtual_mem::PGD_SIZE) {
                        if let Some(leaf) = root_pgd[vpn0(addr)].to_leaf_ref() {
                            drop(unsafe { Box::from_raw(leaf as *mut virtual_mem::PageTable) });
                        }
                        root_pgd[vpn0(addr)].clear();
                    }

                    let sig_ret_phy_addr =
                        virtual_mem::virt_to_phy(thread::sig_ret as *const () as usize);
                    let sig_reg_virt_addr = virtual_mem::USER_MODE_START_ADDRESS
                        + crate::align(curr_file_len, virtual_mem::PAGE_SIZE);
                    virtual_mem::pagewalk(
                        root_pgd as *mut _,
                        sig_reg_virt_addr,
                        sig_ret_phy_addr,
                        virtual_mem::PROT_USER_TEXT,
                    );
                    tcb.sig.sig_ret_addr = sig_reg_virt_addr;

                    tcb.mmap_start_addr = virtual_mem::USER_MODE_START_ADDRESS
                        + crate::align(curr_file_len, virtual_mem::PGD_SIZE);
                    tcb.text_file = Some(file);

                    regs.sepc = virtual_mem::USER_MODE_START_ADDRESS;
                    regs.sscratch = virtual_mem::USER_MODE_STACK_ADDRESS;
                    regs.a0 = 0;
                    regs.ra = thread::u_mode_do_exit as *const () as _;
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

                let tcb = current_tcb();
                tcb.children.remove(&pid);

                let term_queue = &mut tcb.term_children;
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
                let current_lock = current_process.lock();
                let curr_proc = current_lock.get_mut();

                if curr_proc.state == thread::State::Running {
                    curr_proc.state = thread::State::Waiting;

                    let wait_usec: fn(*const u8) = |t: *const u8| {
                        let proc_box: Box<schedule::SafeSendTCB> =
                            unsafe { Box::from_raw(t as *mut _) };
                        let proc_lock = proc_box.lock();
                        let proc = proc_lock.get_mut();

                        if proc.state == thread::State::Waiting {
                            proc.state = thread::State::Ready;
                        }
                        drop(proc_lock);

                        schedule::get_process_ready_queue_mut().push_back(*proc_box);
                    };

                    let args = Box::new(current_process.clone());

                    timer::add_timer(
                        timer::Time::new(delay_usec as _, timer::TimeUnit::MicroSec),
                        wait_usec,
                        Some(args),
                        false,
                    );
                }
                drop(current_lock);

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
                let sig_stack_buttom = sig_stack.as_ptr() as usize;
                let sig_stack_top = sig_stack_buttom + sig_stack.len();
                drop(save);

                if !(prev_regs.sscratch > sig_stack_buttom && prev_regs.sscratch < sig_stack_top) {
                    curr_sig.sig_stack.take();
                    let virt_addr = crate::align(prev_regs.sscratch, virtual_mem::PMD_SIZE)
                        - virtual_mem::PMD_SIZE;
                    let mut pgd = current_tcb().pgd.as_mut().unwrap().as_mut();
                    let pmd =
                        pgd.try_new_entry(virtual_mem::vpn0(virt_addr), virtual_mem::PMD_SHIFT);
                    pmd[virtual_mem::vpn1(virt_addr)] = virtual_mem::PageTableEntry(0);
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
                            drop(lock);
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
                let flags = regs.a3;

                let start_addr = if addr.is_null()
                    || (virtual_mem::vpn2(addr as _)
                        ..=virtual_mem::vpn2(addr as usize + length - 1))
                        .any(|idx| tcb.pgd.as_ref().unwrap()[idx].is_valid())
                    || (addr as usize) < tcb.mmap_start_addr
                {
                    let shift = virtual_mem::virt_shift_align(tcb.mmap_start_addr.trailing_zeros());
                    let start_addr = crate::align(tcb.mmap_start_addr, 1 << shift);
                    tcb.mmap_start_addr = start_addr + length;
                    start_addr
                } else {
                    addr as _
                };

                let mut curr_len = 0;
                let mut curr_shift;
                let root_pgd = tcb.pgd.as_mut().unwrap();

                while curr_len < length {
                    let curr_addr = start_addr + curr_len;

                    let pte = match length - curr_len {
                        ..PMD_SIZE => {
                            curr_shift = PTE_SHIFT;
                            let pmd = root_pgd.try_new_entry(vpn2(curr_addr), PMD_SHIFT);
                            let pte = pmd.try_new_entry(vpn1(curr_addr), PTE_SHIFT);
                            &mut pte[vpn0(curr_addr)]
                        }
                        PMD_SIZE..PGD_SIZE => {
                            curr_shift = PMD_SHIFT;
                            let pmd = root_pgd.try_new_entry(vpn2(curr_addr), PMD_SHIFT);
                            &mut pmd[vpn1(curr_addr)]
                        }
                        PGD_SIZE.. => {
                            curr_shift = PGD_SHIFT;
                            &mut root_pgd[vpn2(curr_addr)]
                        }
                    };

                    let end_size = curr_len + (1 << curr_shift);
                    if flags & MmapFlags::MapAnonymous as usize != 0 {
                        if flags & MmapFlags::MapPopulate as usize != 0 {
                            let page = vec![0u8; 1 << curr_shift];
                            let (ptr, _, _) = Vec::into_raw_parts(page);
                            *pte = PageTableEntry::new(
                                virt_to_phy(ptr as _),
                                prop | PTE_V | PTE_A | PTE_D,
                            );
                        } else {
                            pte.set_prop(prop | PTE_M);
                        }
                    }

                    curr_len = end_size;
                }
                unsafe { asm!("sfence.vma") };
                regs.a0 = start_addr;
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
                use virtual_mem::{phy_to_virt, vpn};
                let fd = regs.a0;
                let req = regs.a1;
                let ptr = regs.a2;

                let tcb = current_tcb();
                let Some(file) = &mut tcb.fdt[fd] else {
                    regs.a0 = -1_isize as _;
                    return;
                };

                let mut curr_shift = virtual_mem::PGD_SHIFT;
                let mut pte_ptr: *mut _ = &mut tcb.pgd.as_mut().unwrap()[vpn(ptr, curr_shift)];

                while let Some(child_pte) = unsafe { &mut *pte_ptr }.to_leaf_ref() {
                    curr_shift -= 9;
                    pte_ptr = &mut child_pte[vpn(ptr, curr_shift)];
                }

                let offset = ptr - (ptr & !((1 << curr_shift) - 1));
                let kernel_ptr = phy_to_virt(unsafe { &*pte_ptr }.get_pa() + offset);

                regs.a0 = match file.ioctl(req, kernel_ptr as _) {
                    Ok(_) => {
                        let _sstatus = SetStatusSUM::new();
                        unsafe {
                            asm!(r#"
                                 cbo.flush ({ker})
                                 cbo.inval ({usr})
                                 fence rw, rw
                                 fence.i
                                 "#,
                             ker = in(reg) kernel_ptr,
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

enum MmapFlags {
    MapAnonymous = 0x20,
    MapPopulate = 0x8000,
}

enum MmapProp {
    None = 0,
    Read = 1,
    Write = 2,
    Exec = 4,
}

impl TryFrom<usize> for MmapFlags {
    type Error = ();
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        match value {
            e if e == Self::MapAnonymous as _ => Ok(Self::MapAnonymous),
            e if e == Self::MapPopulate as _ => Ok(Self::MapPopulate),
            _ => Err(()),
        }
    }
}
