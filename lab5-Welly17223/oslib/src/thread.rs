extern crate alloc;
use crate::{
    interrupt::{self, pt_regs},
    schedule::{self, current_tcb},
    spinlock::SpinLock,
};
use alloc::{boxed::Box, collections::BTreeMap, sync::Arc};
use core::{
    arch::naked_asm,
    sync::atomic::{self, AtomicU32},
};

static ALLOC_PID: AtomicU32 = AtomicU32::new(1);

pub fn alloc_pid() -> u32 {
    ALLOC_PID.fetch_add(1, atomic::Ordering::SeqCst)
}

pub trait ThreadQueue {
    fn push_current(&mut self);
    fn pop(&mut self) -> Option<schedule::SafeSendTCB>;
}

#[derive(Default, Clone, Copy)]
#[repr(C)]
pub struct Context {
    pub ra: usize,
    pub sp: usize,
    pub s: [usize; 12],
}

#[derive(Clone)]
pub struct SigAct {
    pub sig_mask: u64,
    pub sig_handler_func: [usize; u64::BITS as usize],
    pub sig_stack: Option<Box<[u8]>>,
}

impl Default for SigAct {
    fn default() -> Self {
        Self {
            sig_mask: 0,
            sig_handler_func: [0; u64::BITS as _],
            sig_stack: None,
        }
    }
}

#[derive(Clone)]
#[repr(C)]
pub struct ThreadControlTable {
    pub context: Context,
    pub user_init_sp: usize,
    pub awake_time: u64,
    pub exit_code: isize,
    pub state: State,
    pub pid: u32,
    pub ppid: u32,
    pub parent: Option<schedule::SafeSendTCB>,
    pub children: Box<BTreeMap<u32, schedule::SafeSendTCB>>,
    pub term_children: Box<BTreeMap<u32, schedule::SafeSendTCB>>,
    pub kernel_stack: Box<[u8]>,
    pub stack: Option<Box<[u8]>>,
    pub reschedule: bool,
    pub sig: Box<SigAct>,
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
        let lock = init_arc.lock();
        lock.get_mut().term_children.clear();
        drop(lock);

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
    // Make sure to drop the lock before kill the task
    let curr_tcb = schedule::current_tcb();
    curr_tcb.exit_code = code;
    curr_tcb.state = State::Terminate;
    schedule::schedule();

    panic!("This line should not be reached!");
}

impl ThreadControlTable {
    pub fn new(func: *const (), ppid: u32, sstatus: usize) -> Self {
        let kernel_stack = unsafe { Box::<[u8; 0x10000]>::new_zeroed().assume_init() };
        let kernel_stack_top_ptr =
            kernel_stack.as_ptr().wrapping_byte_add(kernel_stack.len()) as usize;

        let mut s = [0; 12];
        let pid = alloc_pid();
        s[0] = func as _;
        s[1] = sstatus;

        // Alloc kernel stack for u mode stack
        let (stack, stack_top_ptr) = if sstatus & (1 << 8) == 0 {
            schedule::USER_THREAD_COUNT.fetch_add(1, atomic::Ordering::Relaxed);
            let stack = unsafe { Box::<[u8; 0x10000]>::new_zeroed().assume_init() };
            let stack_top_ptr = stack.as_ptr().wrapping_byte_add(stack.len()) as usize;
            s[2] = stack_top_ptr;
            s[3] = u_mode_do_exit as *const () as _;

            (Some(stack as _), stack_top_ptr)
        } else {
            s[3] = do_exit as *const () as _;
            (None, 0)
        };

        Self {
            context: Context {
                ra: init_thread as *const () as _,
                sp: kernel_stack_top_ptr,
                s,
            },
            state: State::New,
            children: Box::new(BTreeMap::new()),
            term_children: Box::new(BTreeMap::new()),
            parent: Some(schedule::curr_thread_arc()),
            ppid,
            awake_time: 0,
            pid,
            exit_code: 0,
            kernel_stack,
            stack,
            user_init_sp: stack_top_ptr,
            reschedule: false,
            sig: Box::new(SigAct::default()),
        }
    }

    pub fn from_stack<T: Into<Box<[u8]>>>(exist_stack: T, func: *const (), ppid: u32) -> Self {
        let kernel_stack = unsafe { Box::<[u8; 0x10000]>::new_zeroed().assume_init() };
        let kernel_stack_top_ptr =
            kernel_stack.as_ptr().wrapping_byte_add(kernel_stack.len()) as usize;

        let mut s = [0; 12];
        let pid = alloc_pid();
        s[0] = func as _;
        s[1] = 1 << 5;

        // Alloc kernel stack for u mode stack
        let (stack, stack_top_ptr) = {
            schedule::USER_THREAD_COUNT.fetch_add(1, atomic::Ordering::Relaxed);
            let stack = exist_stack.into();
            let stack_top_ptr = stack.as_ptr().wrapping_byte_add(stack.len()) as usize;
            s[2] = stack_top_ptr;
            s[3] = u_mode_do_exit as *const () as _;

            (Some(stack as _), stack_top_ptr)
        };
        Self {
            context: Context {
                ra: init_thread as *const () as _,
                sp: kernel_stack_top_ptr,
                s,
            },
            state: State::New,
            children: Box::new(BTreeMap::new()),
            term_children: Box::new(BTreeMap::new()),
            parent: Some(schedule::curr_thread_arc()),
            ppid,
            awake_time: 0,
            pid,
            exit_code: 0,
            kernel_stack,
            stack,
            user_init_sp: stack_top_ptr,
            reschedule: false,
            sig: Box::new(SigAct::default()),
        }
    }

    pub fn create(func: *const (), ppid: u32, sstatus: usize) -> u32 {
        let thread = Self::new(func, ppid, sstatus);
        let pid = thread.pid;
        let thread = Arc::new(SpinLock::new(thread));
        schedule::get_process_ready_queue_mut().push_back(thread.clone());
        schedule::get_live_proc()
            .lock()
            .get_mut()
            .insert(pid, thread);
        pid
    }

    pub fn create_thread(thread: Self) -> u32 {
        let pid = thread.pid;
        let thread = Arc::new(SpinLock::new(thread));
        schedule::get_process_ready_queue_mut().push_back(thread.clone());
        schedule::get_live_proc()
            .lock()
            .get_mut()
            .insert(pid, thread);
        pid
    }

    pub fn fork(&mut self, regs: &pt_regs) -> u32 {
        let mut children = self.clone();
        let mut children_regs = *regs;

        let children_stack = children.stack.as_ref().unwrap();
        let offset = current_tcb().user_init_sp - regs.sscratch;
        let pid = alloc_pid();

        children.children = Box::new(BTreeMap::new());
        children.user_init_sp = children_stack.as_ptr().wrapping_add(children_stack.len()) as _;
        children_regs.sscratch = children.user_init_sp - offset;

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
            *(children.context.sp as *mut interrupt::pt_regs) = children_regs;
        }

        let children = Arc::new(SpinLock::new(children));

        self.children.insert(pid, children.clone());
        schedule::get_live_proc()
            .lock()
            .get_mut()
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
    queue: alloc::collections::VecDeque<schedule::SafeSendTCB>,
}

impl ThreadQueue for WaitQueue {
    fn push_current(&mut self) {
        let tcb_arc = schedule::curr_thread_arc();
        let tcb_lock = tcb_arc.lock();
        let tcb = tcb_lock.get_mut();

        if tcb.state == State::Running {
            tcb.state = State::Waiting;
        }
        drop(tcb_lock);

        self.queue.push_back(tcb_arc);
    }

    fn pop(&mut self) -> Option<schedule::SafeSendTCB> {
        self.queue.pop_front()
    }
}
