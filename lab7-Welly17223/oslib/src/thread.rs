extern crate alloc;
use crate::{
    file_system::{self, OpenFlags},
    interrupt::{self, pt_regs},
    schedule::{self, curr_thread_arc},
    virtual_mem::{
        self, VirtualAddress,
        vm_area::{self, Provider},
    },
};
use alloc::{
    boxed::Box,
    collections::BTreeMap,
    sync::{Arc, Weak},
};
use core::{
    arch::naked_asm,
    ptr,
    sync::atomic::{self, AtomicU32},
};
use spin::Mutex;

pub const SIG_STACK_SIZE: usize = virtual_mem::PMD_SIZE;
static ALLOC_PID: AtomicU32 = AtomicU32::new(1);

pub fn alloc_pid() -> u32 {
    ALLOC_PID.fetch_add(1, atomic::Ordering::SeqCst)
}

pub trait ThreadQueue {
    fn push_current(&mut self);
    fn pop(&mut self) -> Option<schedule::SafeSendTCB>;
}

unsafe extern "C" {
    pub static __user_text_start: usize;
    pub static __user_text_end: usize;
}

#[derive(Default, Clone, Copy)]
#[repr(C)]
pub struct Context {
    pub ra: usize,
    pub sp: usize,
    pub s: [usize; 12],
    pub satp: usize,
}

#[derive(Default, Clone, Copy)]
pub struct TextArea {
    pub base: usize,
    pub target: &'static [u8],
}

#[derive(Clone)]
pub struct SigAct {
    pub sig_mask: u64,
    pub sig_handler_func: [usize; u64::BITS as usize],
    pub sig_stack: Option<VirtualAddress>,
    pub sig_ret_addr: usize,
}

impl Default for SigAct {
    fn default() -> Self {
        Self {
            sig_mask: 0,
            sig_handler_func: [0; u64::BITS as _],
            sig_stack: None,
            sig_ret_addr: 0,
        }
    }
}

#[derive(Clone)]
#[repr(C)]
pub struct ThreadControlTable {
    pub context: Context,
    pub awake_time: u64,
    pub user_init_sp: VirtualAddress,
    pub exit_code: isize,
    pub state: State,
    pub pid: u32,
    pub ppid: u32,
    pub parent: Option<schedule::SafeSendTCB>,
    pub children: Box<BTreeMap<u32, schedule::SafeSendTCB>>,
    pub term_children: Box<BTreeMap<u32, schedule::SafeSendTCB>>,
    pub kernel_stack: Box<[u8]>,
    pub vm_mapper: Option<Box<vm_area::Manager>>,
    pub reschedule: bool,
    pub sig: Box<SigAct>,
    // FileDescribeTable
    pub fdt: file_system::FileDescribeTable,
    // Curren work Directory
    pub cwd: file_system::VNode,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum State {
    New,
    Ready,
    Waiting,
    Running,
    Terminate,
}

pub fn idle_thread() -> ! {
    let init_arc = schedule::get_init_thread();
    loop {
        let mut lock = init_arc.lock();
        lock.term_children.clear();
        drop(lock);

        let disable = crate::interrupt::SModeInterrupt::new();

        if schedule::get_process_ready_queue().is_empty() {
            unsafe {
                core::arch::asm!("wfi");
            }
        }

        drop(disable);
        schedule::schedule();
    }
}

impl PartialEq for ThreadControlTable {
    fn eq(&self, other: &Self) -> bool {
        self.pid.eq(&other.pid)
    }
}

impl PartialOrd for ThreadControlTable {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for ThreadControlTable {}

impl Ord for ThreadControlTable {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.pid.cmp(&other.pid)
    }
}

impl Drop for ThreadControlTable {
    fn drop(&mut self) {
        use crate::uart;
        use core::fmt::Write;
        writeln!(uart::get_serial(), "Drop TCB {}", self.pid).unwrap();
    }
}

/// This function invoke exit system call for u mode thread
#[unsafe(naked)]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.user")]
pub extern "C" fn u_mode_do_exit(code: isize) -> ! {
    naked_asm!(
        r#"
        li a7, 6
        ecall
        "#
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn do_exit(code: isize) -> ! {
    let arc = curr_thread_arc();
    unsafe { arc.force_unlock() };
    drop(arc);
    // Make sure to drop the lock before kill the task
    let curr_tcb = schedule::current_tcb();
    curr_tcb.exit_code = code;
    curr_tcb.state = State::Terminate;
    schedule::schedule();

    panic!("This line should not be reached!");
}

impl ThreadControlTable {
    pub fn new_kernel_thread(func: *const (), ppid: u32, sstatus: usize) -> Self {
        let kernel_stack = unsafe { Box::<[u8; 0x100000]>::new_zeroed().assume_init() };
        let kernel_stack_top_ptr =
            kernel_stack.as_ptr().wrapping_byte_add(kernel_stack.len()) as usize;

        let mut s = [0; 12];
        let pid = alloc_pid();
        s[0] = func as _;
        s[1] = sstatus;

        let stack_top_ptr = VirtualAddress(0);
        s[3] = do_exit as *const () as _;

        Self {
            context: Context {
                ra: init_thread as *const () as _,
                sp: kernel_stack_top_ptr,
                s,
                satp: virtual_mem::make_satp(virtual_mem::virt_to_phy(
                    (&raw const virtual_mem::PGD as usize).into(),
                )),
            },
            state: State::New,
            vm_mapper: None,
            children: Box::new(BTreeMap::new()),
            term_children: Box::new(BTreeMap::new()),
            parent: Some(schedule::curr_thread_arc()),
            ppid,
            awake_time: 0,
            pid,
            exit_code: 0,
            kernel_stack,
            user_init_sp: stack_top_ptr,
            // mmap_start_addr: virtual_mem::USER_MODE_STACK_ADDRESS,
            reschedule: false,
            sig: Box::new(SigAct::default()),
            cwd: file_system::ROOT.get().unwrap().root().unwrap(),
            fdt: file_system::FileDescribeTable::default(),
            // text: TextArea::default(),
        }
    }

    pub fn new_user_thread(file: file_system::File, _func: *const (), ppid: u32) -> Self {
        let kernel_stack =
            unsafe { Box::<[u8; virtual_mem::PMD_SIZE]>::new_zeroed().assume_init() };
        let kernel_stack_top_ptr =
            kernel_stack.as_ptr().wrapping_byte_add(kernel_stack.len()) as usize;

        let mut vm_mapper = vm_area::Manager::new();
        // let mut program_pgd = Box::new(virtual_mem::root_pgd_clone());

        vm_mapper
            .map_file_addr(
                virtual_mem::VirtualAddress(0),
                file,
                virtual_mem::PROT_USER_TEXT,
            )
            .unwrap();
        // virtual_mem::load_user_program(&mut program_pgd, user_program.as_ref());

        let mut sig = SigAct::default();

        let user_text = unsafe {
            &*ptr::slice_from_raw_parts(
                &raw const __user_text_start as *const u8,
                (&raw const __user_text_end) as usize - (&raw const __user_text_start) as usize,
            )
        };

        let sig_ret_func = sig_ret as *const () as usize;
        let sig_ret_func_offset = sig_ret_func & 0xfff;
        let user_do_exit_offset = (u_mode_do_exit as *const () as usize) & 0xfff;

        let user_text_base = vm_mapper
            .map(
                user_text.len(),
                virtual_mem::PROT_USER_TEXT,
                Provider::Mem(user_text),
            )
            .unwrap();

        sig.sig_ret_addr = user_text_base.addr() + sig_ret_func_offset;
        vm_mapper
            .map_addr(
                virtual_mem::VirtualAddress(
                    virtual_mem::USER_MODE_STACK_ADDRESS.addr() - 2 * virtual_mem::PMD_SIZE,
                ),
                2 * virtual_mem::PMD_SIZE,
                virtual_mem::PROT_USER_STACK,
                vm_area::Provider::Anonymous,
            )
            .unwrap();

        let mut s = [0; 12];
        let pid = alloc_pid();
        s[0] = virtual_mem::USER_MODE_START_ADDRESS.addr();
        s[1] = 1 << 5;
        s[2] = virtual_mem::USER_MODE_STACK_ADDRESS.addr();
        s[3] = user_text_base.addr() + user_do_exit_offset;

        // file system for stdin, stdout, stderr
        let mut fdt = file_system::FileDescribeTable::default();
        fdt.open("/dev/uart", OpenFlags::from("r")).unwrap();
        fdt.open("/dev/uart", OpenFlags::from("w")).unwrap();
        fdt.open("/dev/uart", OpenFlags::from("w")).unwrap();

        Self {
            context: Context {
                ra: init_thread as *const () as _,
                sp: kernel_stack_top_ptr,
                s,
                satp: vm_mapper.satp(),
            },
            state: State::New,
            vm_mapper: Some(Box::new(vm_mapper)),
            children: Box::new(BTreeMap::new()),
            term_children: Box::new(BTreeMap::new()),
            parent: Some(schedule::curr_thread_arc()),
            ppid,
            awake_time: 0,
            pid,
            exit_code: 0,
            kernel_stack,
            user_init_sp: virtual_mem::USER_MODE_STACK_ADDRESS,
            reschedule: false,
            sig: Box::new(sig),
            cwd: file_system::ROOT.get().unwrap().root().unwrap(),
            fdt,
        }
    }

    pub fn create(func: *const (), ppid: u32, sstatus: usize) -> u32 {
        let thread = Self::new_kernel_thread(func, ppid, sstatus);
        let pid = thread.pid;
        let thread = Arc::new(Mutex::new(thread));
        schedule::get_process_ready_queue_mut().push_back(thread.clone());
        schedule::get_live_proc().lock().insert(pid, thread);
        pid
    }

    pub fn create_thread(thread: Self) -> u32 {
        let pid = thread.pid;
        let thread = Arc::new(Mutex::new(thread));
        schedule::get_process_ready_queue_mut().push_back(thread.clone());
        schedule::get_live_proc().lock().insert(pid, thread);
        pid
    }

    pub fn fork(&mut self, regs: &pt_regs) -> u32 {
        let mut children = self.clone();
        let children_regs = *regs;

        let pid = alloc_pid();

        children.children.clear();
        children.user_init_sp = virtual_mem::USER_MODE_STACK_ADDRESS;

        let child_vm_mapper = children.vm_mapper.as_mut().unwrap();
        children.context.satp = child_vm_mapper.satp();
        child_vm_mapper.pgd.set_fork_prop(0..256);

        children.state = State::Ready;
        children.pid = pid;
        children.ppid = self.pid;

        children.parent = Some(schedule::curr_thread_arc());

        children.context.ra = fork_ret as *const () as _;
        children.context.sp = children
            .kernel_stack
            .as_ptr()
            .wrapping_add(children.kernel_stack.len() - size_of::<interrupt::pt_regs>())
            as _;

        unsafe {
            (children.context.sp as *mut interrupt::pt_regs).write(children_regs);
        }

        let children = Arc::new(Mutex::new(children));

        self.children.insert(pid, children.clone());
        schedule::get_live_proc()
            .lock()
            .insert(pid, children.clone());
        schedule::get_process_ready_queue_mut().push_back(children);

        pid
    }
}

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "C" fn fork_ret() {
    naked_asm!(
        "addi s0, sp, 8 * 35",
        "csrw sscratch, s0",
        "ld s1,  8 * 31(sp)",
        "ld s2,  8 * 32(sp)",
        "csrw sepc, s1",
        "csrw sstatus, s2",
        "ld ra,  8 *  0(sp)",
        "ld gp,  8 *  2(sp)",
        // "ld tp,  8 *  3(sp)",
        "ld t0,  8 *  4(sp)",
        "ld t1,  8 *  5(sp)",
        "ld t2,  8 *  6(sp)",
        "ld s0,  8 *  7(sp)",
        "ld s1,  8 *  8(sp)",
        // fork children return 0
        "mv a0,  zero",
        "ld a1,  8 * 10(sp)",
        "ld a2,  8 * 11(sp)",
        "ld a3,  8 * 12(sp)",
        "ld a4,  8 * 13(sp)",
        "ld a5,  8 * 14(sp)",
        "ld a6,  8 * 15(sp)",
        "ld a7,  8 * 16(sp)",
        "ld s2,  8 * 17(sp)",
        "ld s3,  8 * 18(sp)",
        "ld s4,  8 * 19(sp)",
        "ld s5,  8 * 20(sp)",
        "ld s6,  8 * 21(sp)",
        "ld s7,  8 * 22(sp)",
        "ld s8,  8 * 23(sp)",
        "ld s9,  8 * 24(sp)",
        "ld s10, 8 * 25(sp)",
        "ld s11, 8 * 26(sp)",
        "ld t3,  8 * 27(sp)",
        "ld t4,  8 * 28(sp)",
        "ld t5,  8 * 29(sp)",
        "ld t6,  8 * 30(sp)",
        "ld sp,  8 *  1(sp)",
        "sret",
    )
}

/// init_thread
/// # Safety
///
/// threadinit
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn init_thread() {
    core::arch::naked_asm!(
        r#"
            csrw sepc, s0
            csrw sstatus, s1
            csrw sscratch, s2
            mv ra, s3
            // check the return mode if is u mode, then switch the kernel stack and user stack
            andi s1, s1, (1 << 8)
            bne s1, zero, s_mode
            csrrw sp, sscratch, sp
        s_mode:
            sret
        "#
    );
}

#[unsafe(naked)]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.user")]
/// # Safety
/// only use when return from a userspace signal handler
pub unsafe extern "C" fn sig_ret() {
    naked_asm!(
        r#"
        li a7, 11
        ecall
        "#
    )
}

#[derive(Default)]
pub struct WaitQueue {
    queue: alloc::collections::VecDeque<Weak<Mutex<ThreadControlTable>>>,
}

impl ThreadQueue for WaitQueue {
    fn push_current(&mut self) {
        let tcb_arc = schedule::curr_thread_arc();
        let mut tcb = tcb_arc.lock();

        if tcb.state == State::Running {
            tcb.state = State::Waiting;
        }
        drop(tcb);

        self.queue.push_back(Arc::downgrade(&tcb_arc));
    }

    fn pop(&mut self) -> Option<schedule::SafeSendTCB> {
        loop {
            let weak = self.queue.pop_front()?;
            let arc = weak.upgrade();
            if arc.is_some() {
                break arc;
            }
        }
    }
}
