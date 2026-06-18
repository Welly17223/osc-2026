use crate::interrupt::timer::offset_sec;
use crate::{fdt, interrupt::plic::IRQ, uart::SERIAL};
use core::arch::asm;
use core::cmp::Ordering;
use core::{ffi, panic};
extern crate alloc;
use alloc::boxed::Box;
use alloc::string::String;
use core::fmt::{Display, Write};

pub mod input_handler;
pub mod plic;
pub mod timer;

pub static IRQ_TABLE: spin::RwLock<Option<alloc::collections::BTreeMap<u32, IRQ>>> =
    spin::RwLock::new(None);

type InputFunc = fn(String, &mut pt_regs) -> Option<TaskEntry>;
pub static INPUT_HANDLER: spin::RwLock<Option<InputFunc>> = spin::RwLock::new(None);
static mut INTERRUPT_TASK: Option<pt_regs> = None;

pub enum SupervisorInterrupt1 {
    Software,
    Timer,
    External,
    CounterOverflow,
}

#[derive(Debug)]
pub enum SupervisorInterrupt0 {
    InstructionAddressMisaligned,
    InstructionAccessFault,
    IllegalInstruction,
    Breakpoint,
    LoadAddressMisaligned,
    LoadAccessFault,
    StoreAMOAddressMisaligned,
    StoreAMOAccessFault,
    EnvironmentCallFromUmode,
    EnvironmentCallFromSmode,
    InstructionPageFault,
    LoadPageFault,
    StoreAMOPageFault,
    SoftwareCheck,
    HardwareError,
    DesignatedForCustomuse,
    Reserved,
}

pub struct SModeInterrupt {
    is_enable: bool,
}

impl SModeInterrupt {
    pub fn new() -> Self {
        let is_enable = s_mode_interrupt_status();
        s_mode_interrupt_disable();
        SModeInterrupt { is_enable }
    }

    pub fn set(state: bool) -> Self {
        let is_enable = s_mode_interrupt_status();
        if state {
            s_mode_interrupt_enable();
        } else {
            s_mode_interrupt_disable();
        }
        Self { is_enable }
    }
}

impl Default for SModeInterrupt {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SModeInterrupt {
    fn drop(&mut self) {
        if self.is_enable {
            s_mode_interrupt_enable();
        } else {
            s_mode_interrupt_disable();
        }
    }
}

impl TryFrom<usize> for SupervisorInterrupt1 {
    type Error = ();
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Software),
            5 => Ok(Self::Timer),
            9 => Ok(Self::External),
            13 => Ok(Self::CounterOverflow),
            _ => Err(()),
        }
    }
}

impl From<SupervisorInterrupt1> for u32 {
    fn from(value: SupervisorInterrupt1) -> Self {
        match value {
            SupervisorInterrupt1::Software => 1,
            SupervisorInterrupt1::Timer => 5,
            SupervisorInterrupt1::External => 9,
            SupervisorInterrupt1::CounterOverflow => 13,
        }
    }
}

impl From<usize> for SupervisorInterrupt0 {
    fn from(value: usize) -> Self {
        match value {
            0 => SupervisorInterrupt0::InstructionAddressMisaligned,
            1 => SupervisorInterrupt0::InstructionAccessFault,
            2 => SupervisorInterrupt0::IllegalInstruction,
            3 => SupervisorInterrupt0::Breakpoint,
            4 => SupervisorInterrupt0::LoadAddressMisaligned,
            5 => SupervisorInterrupt0::LoadAccessFault,
            6 => SupervisorInterrupt0::StoreAMOAddressMisaligned,
            7 => SupervisorInterrupt0::StoreAMOAccessFault,
            8 => SupervisorInterrupt0::EnvironmentCallFromUmode,
            12 => SupervisorInterrupt0::EnvironmentCallFromSmode,
            13 => SupervisorInterrupt0::InstructionPageFault,
            14 => SupervisorInterrupt0::LoadPageFault,
            15 => SupervisorInterrupt0::StoreAMOPageFault,
            18 => SupervisorInterrupt0::SoftwareCheck,
            19 => SupervisorInterrupt0::HardwareError,
            24..=31 | 48..=63 => SupervisorInterrupt0::DesignatedForCustomuse,
            _ => SupervisorInterrupt0::Reserved,
        }
    }
}

pub type TaskCallback = extern "C" fn(*const u8);
pub type TaskCallbackCFunct = unsafe extern "C" fn(*const ffi::c_void);
pub static mut CURRENT_TASK: Option<TaskEntry> = None;
pub static mut TASK_QUEUE: Option<alloc::collections::BinaryHeap<TaskEntry>> = None;

#[derive(Debug)]
pub struct TaskEntry {
    callback: TaskCallback,
    args: *const u8,
    args_type: Args,
    priority: u32,
    id: u64,
}

#[derive(Debug)]
enum Args {
    CArgs,
    RustArgs,
}

impl Display for TaskEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}, {} {:?}", self.id, self.priority, self.callback)?;
        Ok(())
    }
}

impl PartialEq for TaskEntry {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl PartialOrd for TaskEntry {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Eq for TaskEntry {}
impl Ord for TaskEntry {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        let tmp_res = self.priority.cmp(&other.priority);
        if let Ordering::Equal = tmp_res {
            self.id.cmp(&other.id)
        } else {
            tmp_res
        }
    }
}

impl Drop for TaskEntry {
    fn drop(&mut self) {
        if let Args::RustArgs = self.args_type {
            let _drop = unsafe { Box::from_raw(self.args as *mut u8) };
        }
    }
}

impl TaskEntry {
    pub fn new<T>(callback: TaskCallback, args: Box<T>, priority: u32) -> Self {
        Self {
            callback,
            args: Box::into_raw(args) as *const u8,
            args_type: Args::RustArgs,
            priority,
            id: timer::get_time_raw(),
        }
    }

    pub fn new_c(callback: TaskCallback, args: *const u8, priority: u32) -> Self {
        Self {
            callback,
            args,
            args_type: Args::CArgs,
            priority,
            id: timer::get_time_raw(),
        }
    }

    #[inline]
    pub fn id(&self) -> u64 {
        self.id
    }

    #[inline]
    pub fn update_time(&mut self) {
        self.id = timer::get_time_raw();
    }

    #[inline]
    pub fn set_time(&mut self, time: u64) {
        self.id = time;
    }

    pub fn priority(&self) -> u32 {
        self.priority
    }
}

pub fn add_task<T>(callback: TaskCallback, args: Box<T>, priority: u32) {
    let task_queue_ptr = &raw mut TASK_QUEUE;
    let Some(task_queue) = (unsafe { &mut *task_queue_ptr }) else {
        return;
    };

    let task = TaskEntry::new(callback, args, priority);
    task_queue.push(task);
}

#[unsafe(no_mangle)]
pub extern "C" fn add_task_c(
    callback: TaskCallback,
    args: *const ffi::c_void,
    priority: ffi::c_int,
) {
    let task_queue_ptr = &raw mut TASK_QUEUE;
    let Some(task_queue) = (unsafe { &mut *task_queue_ptr }) else {
        return;
    };

    let task = TaskEntry::new_c(callback, args as *const u8, priority as u32);
    task_queue.push(task);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(C)]
pub struct pt_regs {
    pub ra: usize,
    pub sscratch: usize,
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
    pub sepc: usize,
    pub sstatus: usize,
    pub scause: usize,
    pub stval: usize,
}

impl From<crate::schedule::RiscVRegs> for pt_regs {
    fn from(value: crate::schedule::RiscVRegs) -> Self {
        let mut regs = Self::default();
        regs.set_from(value);
        regs
    }
}

impl pt_regs {
    pub fn set_from(&mut self, regs: crate::schedule::RiscVRegs) {
        self.ra = regs.ra;
        self.sscratch = regs.sp;
        self.gp = regs.gp;
        self.tp = regs.tp;
        self.t0 = regs.t0;
        self.t1 = regs.t1;
        self.t2 = regs.t2;
        self.s0 = regs.s0;
        self.s1 = regs.s1;
        self.a0 = regs.a0;
        self.a1 = regs.a1;
        self.a2 = regs.a2;
        self.a3 = regs.a3;
        self.a4 = regs.a4;
        self.a5 = regs.a5;
        self.a6 = regs.a6;
        self.a7 = regs.a7;
        self.s2 = regs.s2;
        self.s3 = regs.s3;
        self.s4 = regs.s4;
        self.s5 = regs.s5;
        self.s6 = regs.s6;
        self.s7 = regs.s7;
        self.s8 = regs.s8;
        self.s9 = regs.s9;
        self.s10 = regs.s10;
        self.s11 = regs.s11;
        self.t3 = regs.t3;
        self.t4 = regs.t4;
        self.t5 = regs.t5;
        self.t6 = regs.t6;
    }
}

static mut CURRENT_PRIORITY: Option<u32> = None;

fn task_scheduler() {
    let curr_priority = &raw mut CURRENT_PRIORITY;
    let curr_priority = unsafe { &mut *curr_priority };
    let curr_queue = &raw mut TASK_QUEUE;
    let curr_queue = unsafe { &mut *curr_queue }.as_mut().unwrap();

    let mut hightest_task_option = curr_queue.peek();
    while let Some(highest_task) = hightest_task_option {
        match curr_priority {
            Some(priority) if *priority < highest_task.priority => {
                let highest_task = curr_queue.pop().unwrap();

                let tmp = *priority;
                unsafe { CURRENT_PRIORITY = Some(highest_task.priority) };

                let enable_interrupt = SModeInterrupt::set(true);
                (highest_task.callback)(highest_task.args);
                drop(enable_interrupt);

                unsafe { CURRENT_PRIORITY = Some(tmp) };
            }
            Some(_) => {
                break;
            }
            None => {
                let highest_task = curr_queue.pop().unwrap();
                unsafe { CURRENT_PRIORITY = Some(highest_task.priority) };

                let enable_interrupt = SModeInterrupt::set(true);
                (highest_task.callback)(highest_task.args);
                drop(enable_interrupt);

                unsafe { CURRENT_PRIORITY = None };
            }
        }

        hightest_task_option = curr_queue.peek();
    }
}

#[unsafe(no_mangle)]
extern "C" fn do_trap(regs: *mut pt_regs) {
    // after this element is dropped, the interrupt is enable

    let regs = unsafe { &mut *regs };
    let interrupt = regs.scause >> (usize::BITS - 1);
    let serial = &raw mut SERIAL;

    let interrupt_process_ptr = &raw mut INTERRUPT_TASK;
    if unsafe { &*interrupt_process_ptr }.is_none() {
        unsafe { INTERRUPT_TASK = Some(*regs) };
    }

    if interrupt == 1 {
        match (regs.scause & ((1 << (usize::BITS - 2)) - 1)).try_into() {
            Ok(SupervisorInterrupt1::Software) => {
                clear_software_pendding_bit();
            }
            Ok(SupervisorInterrupt1::Timer) => {
                clear_timer_pending_bit();
                let time_queue = &raw mut timer::TIMER_QUEUE;
                let Some(timer_queue) = (unsafe { &mut *time_queue }) else {
                    return;
                };
                let Some(serial) = (unsafe { &mut *serial }) else {
                    return;
                };
                let curr_time = timer::get_time_raw();

                while let Some(entry) = timer_queue.peek()
                    && entry.0.get_time() < curr_time
                {
                    let entry = timer_queue.pop().unwrap().0;
                    (entry.f)(entry.args);
                    if let Some(new_entry) = entry.next_repeat() {
                        timer_queue.push(core::cmp::Reverse(new_entry));
                    }
                }
                if let Some(entry) = timer_queue.peek() {
                    let entry = &entry.0;
                    timer::set_timer_raw(entry.get_time());
                }
            }
            Ok(SupervisorInterrupt1::External) => {
                let plic_raw = &raw const plic::HART0_PLIC;
                let Some(plic) = (unsafe { &*plic_raw }) else {
                    return;
                };
                let irq = plic.claim(0);
                let Some(irq_table) = &*IRQ_TABLE.read() else {
                    return;
                };

                match irq_table.get(&irq) {
                    Some(IRQ::UART) => {
                        let Some(serial) = (unsafe { &mut *serial }) else {
                            return;
                        };
                        let iir_ise = (serial.get_iir() >> 1) & 0b11;

                        match iir_ise {
                            // transmit fifo requests
                            1 => serial.tx_interrupt(),
                            // received data avaliable
                            2 => {
                                serial.push_rx(serial.read_lsr() as u8);
                            }
                            _ => {}
                        }
                    }
                    // Some(_) => {}
                    None => (),
                }
                plic.complete(0, irq);
            }
            Ok(SupervisorInterrupt1::CounterOverflow) => {}
            Err(_) => (),
        }
    } else {
        let Some(serial) = (unsafe { &mut *serial }) else {
            return;
        };
        match regs.scause.into() {
            SupervisorInterrupt0::EnvironmentCallFromUmode => {
                writeln!(serial, "=== S-Mode trap ===").unwrap();
                writeln!(
                    serial,
                    "scause: {}\nsepc: {:#x}\nstval: {}",
                    regs.scause, regs.sepc, regs.stval
                )
                .unwrap();
                regs.sepc += 4;
            }
            SupervisorInterrupt0::InstructionAddressMisaligned
            | SupervisorInterrupt0::InstructionAccessFault
            | SupervisorInterrupt0::IllegalInstruction
            | SupervisorInterrupt0::LoadAddressMisaligned
            | SupervisorInterrupt0::StoreAMOAddressMisaligned
            | SupervisorInterrupt0::StoreAMOAccessFault
            | SupervisorInterrupt0::InstructionPageFault
            | SupervisorInterrupt0::LoadPageFault
            | SupervisorInterrupt0::HardwareError => {
                serial.puts("scause: ");
                serial.put_hex(regs.scause as u64);
                serial.puts(" sepc: ");
                serial.put_hex(regs.sepc as u64);
                serial.puts(" stval: ");
                serial.put_hex(regs.stval as u64);
                serial.putc(b'\n');
                panic!("{} {} {}", regs.scause, regs.sepc, regs.stval);
            }
            interrupt => writeln!(serial, "{:?} not handle", interrupt).unwrap(),
        }
    }

    task_scheduler();
}

pub fn interrupt_init(dtb_addr: *mut u8, hart_id: usize) -> Result<(), fdt::Error> {
    *IRQ_TABLE.write() = Some(alloc::collections::BTreeMap::new());
    unsafe { TASK_QUEUE = Some(alloc::collections::BinaryHeap::new()) };

    plic::init_plic(dtb_addr, hart_id)?;
    enable_external_interrupt();
    enable_timer_interrupt();
    enable_software_interrupt();
    unsafe {
        asm!("la {tmp}, handle_exception",
            "csrw stvec, {tmp}",
            tmp = out(reg) _ );
    }
    timer::init_frequency(dtb_addr);
    timer::set_timer_raw(offset_sec(10));
    unsafe {
        asm!("fence io, io", "fence.i");
    }

    Ok(())
}

pub fn enable_software_interrupt() {
    unsafe {
        // sie | 1 << 1
        riscv::register::sie::set_ssoft();
    }
}

pub fn trigger_software_interrupt() {
    unsafe {
        // sip | 1 << 1
        riscv::register::sip::set_ssoft();
    }
}

pub fn clear_software_pendding_bit() {
    unsafe {
        // sip & !(1 << 1)
        riscv::register::sip::clear_ssoft();
    }
}

pub fn enable_timer_interrupt() {
    unsafe {
        /* asm!("li {tmp}, 32",
            "csrs sie, {tmp}",
            tmp = out(reg) _
        ); */
        riscv::register::sie::set_stimer();
    }
}

pub fn enable_external_interrupt() {
    unsafe {
        /* asm!("li {tmp}, (1<<9)",
            "csrs sie, {tmp}",
            tmp = out(reg) _
        ); */
        riscv::register::sie::set_sext();
    }
}

pub fn s_mode_interrupt_enable() {
    unsafe {
        // asm!("csrsi sstatus, (1 << 1)");
        riscv::register::sstatus::set_sie();
    }
}

pub fn s_mode_interrupt_status() -> bool {
    riscv::register::sstatus::read().sie()
}

pub fn s_mode_interrupt_disable() {
    unsafe {
        // asm!("csrci sstatus, (1 << 1)");
        riscv::register::sstatus::clear_sie();
    }
}

pub fn clear_timer_pending_bit() {
    unsafe {
        asm!("li {tmp}, 32",
             "csrc sip, {tmp}",
             tmp = out(reg) _);
    }
}
