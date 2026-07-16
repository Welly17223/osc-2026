extern crate alloc;
use alloc::{
    boxed::Box,
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};

use core::{
    arch::{self, asm},
    fmt::Write,
    sync::atomic,
};

use crate::{
    file_system,
    interrupt::{
        self,
        timer::{self, Time},
    },
    kernel_shell,
    // spinlock::SpinLock,
    thread::{self, Context, State, ThreadControlTable, alloc_pid},
    uart,
    virtual_mem::{self, VirtualAddress},
};

use spin::{Mutex, Once};

pub type SafeSendTCB = Arc<Mutex<ThreadControlTable>>;

pub const ROUND_ROBIN_TIME_LIMIT_MICRO_SEC: u64 = 1000;

static IS_CONTAXT_SWITCH: atomic::AtomicBool = atomic::AtomicBool::new(false);
pub static USER_THREAD_COUNT: atomic::AtomicU32 = atomic::AtomicU32::new(0);
static mut IDLE_THREAD_TCB: Option<SafeSendTCB> = None;

pub struct Process {}

/// context switch
/// # Safety
///
/// This function only use in schedule function to execute context switch
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn switch_to(prev: *mut Context, next: *mut Context) {
    arch::naked_asm!(
        r#"
        rdtime a2
        sd ra , 8 *   0(a0)
        sd sp , 8 *   1(a0)
        sd s0 , 8 *   2(a0)
        sd s1 , 8 *   3(a0)
        sd s2 , 8 *   4(a0)
        sd s3 , 8 *   5(a0)
        sd s4 , 8 *   6(a0)
        sd s5 , 8 *   7(a0)
        sd s6 , 8 *   8(a0)
        sd s7 , 8 *   9(a0)
        sd s8 , 8 *  10(a0)
        sd s9 , 8 *  11(a0)
        sd s10, 8 *  12(a0)
        sd s11, 8 *  13(a0)"#,
        // save current time
        "sd a2 , 8 *  15(a1)",
        // load satp
        "ld t0,  8 *  14(a1)",
        r#"
        ld ra , 8 *   0(a1)
        ld sp , 8 *   1(a1)
        ld s0 , 8 *   2(a1)
        ld s1 , 8 *   3(a1)
        ld s2 , 8 *   4(a1)
        ld s3 , 8 *   5(a1)
        ld s4 , 8 *   6(a1)
        ld s5 , 8 *   7(a1)
        ld s6 , 8 *   8(a1)
        ld s7 , 8 *   9(a1)
        ld s8 , 8 *  10(a1)
        ld s9 , 8 *  11(a1)
        ld s10, 8 *  12(a1)
        ld s11, 8 *  13(a1)
        mv tp, a1
        csrw satp, t0
        sfence.vma
        ret
        "#
    );
}

pub struct WaitPidQueue {
    queue: BTreeMap<u32, SafeSendTCB>,
}

impl WaitPidQueue {
    pub fn push_current(&mut self, pid: u32) {
        let tcb_arc = curr_thread_arc();
        let mut tcb_lock = tcb_arc.lock();
        let tcb = &mut *tcb_lock;

        if tcb.term_children.contains_key(&pid) {
            return;
        }

        if tcb.state == State::Running {
            tcb.state = State::Waiting;
        }
        drop(tcb_lock);

        self.queue.insert(pid, tcb_arc);
    }

    pub fn pop(&mut self, pid: u32) -> Option<self::SafeSendTCB> {
        self.queue.remove(&pid)
    }
}

pub static mut WAIT_PID_QUEUE: Once<Mutex<WaitPidQueue>> = Once::new();
static mut PROCESS_READY_QUEUE: Once<alloc::collections::VecDeque<SafeSendTCB>> = Once::new();
pub static mut CURR_THREAD: Option<SafeSendTCB> = None;

static mut LIVE_PROC: Once<Mutex<alloc::collections::BTreeMap<u32, SafeSendTCB>>> = Once::new();

pub fn current_tcb() -> &'static mut ThreadControlTable {
    let curr_proc_ptr: *mut ThreadControlTable;
    IS_CONTAXT_SWITCH.store(true, atomic::Ordering::Release);
    unsafe {
        asm!("mv {}, tp", out(reg)curr_proc_ptr);
        &mut *curr_proc_ptr
    }
}

pub fn current_pid() -> u32 {
    current_tcb().pid
}

pub fn current_state() -> State {
    current_tcb().state
}

pub fn curr_thread_arc() -> SafeSendTCB {
    let ptr = &raw const CURR_THREAD;
    match unsafe { &*ptr }.as_ref() {
        Some(t) => t.clone(),
        None => panic!("Not initilized"),
    }
}

pub fn init() {
    let process_ready_queue_ptr = &raw const PROCESS_READY_QUEUE;
    let wait_pid_queue_ptr = &raw const WAIT_PID_QUEUE;
    let live_proc_ptr = &raw const LIVE_PROC;
    unsafe {
        (&*process_ready_queue_ptr).call_once(VecDeque::new);
        (&*wait_pid_queue_ptr).call_once(|| {
            Mutex::new(WaitPidQueue {
                queue: BTreeMap::new(),
            })
        });
        (&*live_proc_ptr).call_once(|| Mutex::new(BTreeMap::new()));
    }

    // Add idle thread pid = 1
    let boot_pid = alloc_pid();
    let boot_idle_thread = Arc::new(Mutex::new(ThreadControlTable {
        context: Context {
            ra: 0,
            sp: 0,
            s: [0; 12],
            satp: virtual_mem::make_satp(
                VirtualAddress({ &raw const virtual_mem::PGD } as _).into_phy(),
            ),
        },
        state: State::Running,
        vm_mapper: None,
        pid: boot_pid,
        exit_code: 0,
        kernel_stack: Box::new([0; 1]),
        children: Box::new(BTreeMap::new()),
        term_children: Box::new(BTreeMap::new()),
        parent: None,
        ppid: 1,
        user_init_sp: 0usize.into(),
        awake_time: 0,
        reschedule: false,
        sig: Box::new(thread::SigAct::default()),
        cwd: file_system::ROOT.get().unwrap().root().unwrap(),
        fdt: file_system::FileDescribeTable::default(),
    }));

    let boot_tcb_ptr = &*boot_idle_thread.as_ref().lock() as *const _ as usize;
    unsafe {
        CURR_THREAD = Some(boot_idle_thread.clone());
        IDLE_THREAD_TCB = Some(boot_idle_thread.clone());
        asm!("mv tp, {}", in(reg) boot_tcb_ptr);
    }

    // Add kernel shell, pid = 2
    let kernel_shell = thread::ThreadControlTable::new_kernel_thread(
        kernel_shell::control_input as _,
        boot_pid,
        (1 << 5) | (1 << 8),
    );
    let kernel_shell_pid = kernel_shell.pid;
    let kernel_shell = Arc::new(Mutex::new(kernel_shell));
    get_live_proc()
        .lock()
        .insert(kernel_shell_pid, kernel_shell.clone());
    get_process_ready_queue_mut().push_back(kernel_shell);

    // Thread preempt
    timer::add_timer::<u8>(
        Time::new(ROUND_ROBIN_TIME_LIMIT_MICRO_SEC, timer::TimeUnit::MicroSec),
        |_arg| {
            if timer::get_time_raw() - current_tcb().awake_time
                > Time::new(ROUND_ROBIN_TIME_LIMIT_MICRO_SEC, timer::TimeUnit::MicroSec).as_raw()
            {
                current_tcb().reschedule = true;
            }
        },
        None,
        true,
    );
}

pub fn get_init_thread() -> SafeSendTCB {
    match unsafe { &*(&raw const IDLE_THREAD_TCB) } {
        Some(t) => t.clone(),
        None => panic!("Not initilized"),
    }
}

pub fn get_process_ready_queue_mut() -> &'static mut VecDeque<SafeSendTCB> {
    let ptr = &raw mut PROCESS_READY_QUEUE;
    if let Some(l) = unsafe { &mut *ptr }.get_mut() {
        l
    } else {
        panic!("Not initilized");
    }
}

pub fn get_process_ready_queue() -> &'static VecDeque<SafeSendTCB> {
    let ptr = &raw mut PROCESS_READY_QUEUE;
    if let Some(l) = unsafe { &mut *ptr }.get_mut() {
        l
    } else {
        panic!("Not initilized");
    }
}

pub fn get_waitpid_queue_mut() -> &'static Mutex<WaitPidQueue> {
    let ptr = &raw const WAIT_PID_QUEUE;
    if let Some(l) = unsafe { &*ptr }.get() {
        l
    } else {
        panic!("Not initilized");
    }
}

pub fn get_live_proc() -> &'static Mutex<BTreeMap<u32, SafeSendTCB>> {
    let ptr = &raw const LIVE_PROC;
    match unsafe { &*ptr }.get() {
        Some(t) => t,
        None => panic!("Not initilized"),
    }
}

pub fn kwait_pid(pid: u32) -> isize {
    let mut lock = get_waitpid_queue_mut().lock();
    lock.push_current(pid);
    drop(lock);

    schedule();

    let arc = curr_thread_arc();
    let mut lock = arc.lock();
    lock.children.remove(&pid);

    let term_queue = &mut lock.term_children;
    let children = term_queue.remove(&pid).unwrap();
    children.lock().exit_code
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
    let _disable_interrupt = interrupt::SModeInterrupt::new();
    let thread_queue_ptr = &raw mut PROCESS_READY_QUEUE;
    let thread_queue = unsafe { &mut *thread_queue_ptr }.get_mut().unwrap();
    let curr_process_ptr = &raw mut CURR_THREAD;

    let curr_process_arc = unsafe { &mut *(curr_process_ptr) }.take().unwrap();
    let mut curr_process_lock = curr_process_arc.lock();
    let curr_process = &mut *curr_process_lock;
    let curr_process_ptr = curr_process as *const _;

    match curr_process.state {
        State::New | State::Running /*if curr_process.pid > 1 */=> {
            curr_process.state = State::Ready;
            thread_queue.push_back(curr_process_arc.clone());
        }
        State::Terminate => {
            let init_thread_arc = get_init_thread();
            let mut init_thread_lock = init_thread_arc.lock();
            let init_thread = &mut *init_thread_lock;

            curr_process.children.iter().filter(|(pid, child)| {
                let child = child.lock();
                if child.state == State::Terminate {
                    curr_process.term_children.remove(pid).is_none()
                } else {
                    true
                }
            }).for_each(|(pid, child)| {
                let mut child_thread = child.lock();
                child_thread.ppid = init_thread.pid;
                child_thread.parent = Some(init_thread_arc.clone());

                writeln!(uart::get_serial(), "move orphan {} parent to init!", pid).unwrap();

                init_thread.children.insert(*pid, child.clone());

            });
            curr_process.children.clear();
            curr_process.term_children.clear();

            drop(init_thread_lock);

            // clear data from live thread
            let mut live_queue = get_live_proc().lock();
            live_queue.remove(&curr_process.pid);

            // push self to terminate queue
            let parent_arc = curr_process.parent.as_ref().unwrap().clone();
            let mut parent = parent_arc.lock();
            let parent_children_queue = &mut parent.children;
            parent_children_queue.remove(&curr_process.pid);

            let parent_term_queue = &mut parent.term_children;
            parent_term_queue
                .insert(current_tcb().pid, curr_process_arc.clone());
            drop(parent);

            // check if there are proccess is waiting for the thread
            let waitpid_queue_lock = get_waitpid_queue_mut().lock();
            let mut wait_pid_queue = waitpid_queue_lock;

            if let Some(thread) = wait_pid_queue.pop(current_tcb().pid as _) {
                let mut lock = thread.lock();
                if lock.state == State::Waiting {
                    lock.state = State::Ready;
                }
                drop(lock);

                get_process_ready_queue_mut().push_back(thread);
            }

            // drop memory
            drop(curr_process.vm_mapper.take());
        }
        _ => {}
    }

    drop(curr_process_lock);
    drop(curr_process_arc);

    let next_proc_ptr = match thread_queue.is_empty() {
        true => return,
        false => {
            let next_proc_arc = thread_queue.pop_front().unwrap();
            let mut next_proc = next_proc_arc.lock();
            let next_proc_ptr = &*next_proc as *const _;
            if next_proc.state == State::Ready || next_proc.state == State::New {
                next_proc.state = State::Running;
            }

            drop(next_proc);
            unsafe { CURR_THREAD = Some(next_proc_arc) };
            next_proc_ptr
        }
    };
    unsafe {
        switch_to(curr_process_ptr as *mut _, next_proc_ptr as *mut _);
    }

    if current_state() == State::Terminate {
        schedule();
    }

    drop(_disable_interrupt);
}
