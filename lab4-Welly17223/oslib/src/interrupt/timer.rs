use crate::{fdt, sbi};
use core::arch::asm;
use core::cmp::Reverse;
use core::ptr;
extern crate alloc;
use alloc::boxed::Box;
use alloc::collections::BinaryHeap;

pub const TICK_PER_SEC: u64 = 2400;
pub static mut TICK_CYCLE: u64 = 0;
pub static mut TIMER_QUEUE: Option<BinaryHeap<Reverse<TimerEntry>>> = None;
static mut SSTC: bool = false;

pub fn init_frequency(dtb_addr: *mut u8) {
    let n = fdt::path_offset(dtb_addr, "/cpus", 1).unwrap();
    let (freq_ptr, _) = fdt::getprop(dtb_addr, n, "timebase-frequency").unwrap();
    *TIMEBASE_FREQUENCY.write() = Some(unsafe { *(freq_ptr as *const u32) }.swap_bytes());
    unsafe { TIMER_QUEUE = Some(BinaryHeap::new()) };

    let n = fdt::path_offset(dtb_addr, "/cpus/cpu", 1).unwrap();
    if let Ok((cstr, len)) = fdt::getprop(dtb_addr, n, "riscv,isa-extensions") {
        let extensions =
            unsafe { str::from_utf8_unchecked(&*ptr::slice_from_raw_parts(cstr, len)) };
        if extensions.contains("sstc") {
            unsafe { SSTC = true };
        }
    }

    unsafe { TICK_CYCLE = *(freq_ptr as *const u32) as u64 / TICK_PER_SEC };
}

pub struct Time {
    time: u64,
    unit: TimeUnit,
}

impl Time {
    pub fn new(time: u64, unit: TimeUnit) -> Self {
        Self { time, unit }
    }

    pub fn as_raw(&self) -> u64 {
        match self.unit {
            TimeUnit::Sec => self.time * get_sec(),
            TimeUnit::MicroSec => self.time * get_sec() / 1000,
            TimeUnit::Raw => self.time,
        }
    }

    pub fn as_sec(&self) -> u64 {
        match self.unit {
            TimeUnit::Sec => self.time,
            TimeUnit::MicroSec => self.time / 1000,
            TimeUnit::Raw => self.time / get_sec(),
        }
    }
}
impl PartialEq for Time {
    fn eq(&self, other: &Self) -> bool {
        self.as_raw() == other.as_raw()
    }
}
impl Eq for Time {}

impl PartialOrd for Time {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Time {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.as_raw().cmp(&other.as_raw())
    }
}

pub enum TimeUnit {
    Sec,
    MicroSec,
    Raw,
}

pub struct TimerEntry {
    pub f: fn(*const u8),
    pub args: *const u8,
    time: u64,
    // unit is raw
    repeat: Option<Time>,
}
unsafe impl Send for TimerEntry {}
unsafe impl Sync for TimerEntry {}

impl PartialEq for TimerEntry {
    fn eq(&self, other: &Self) -> bool {
        self.time == other.time
    }
}

impl PartialOrd for TimerEntry {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for TimerEntry {}

impl Ord for TimerEntry {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.time.cmp(&other.time)
    }
}

impl TimerEntry {
    pub fn new(func: fn(*const u8), args: *const u8, time_offset: Time, repeat: bool) -> Self {
        Self {
            f: func,
            args,
            time: get_time_raw() + time_offset.as_raw(),
            repeat: if repeat { Some(time_offset) } else { None },
        }
    }

    pub fn repeat_sec(&self) -> Option<&Time> {
        self.repeat.as_ref()
    }

    pub fn next_repeat(mut self) -> Option<Self> {
        let repeat = self.repeat.as_ref()?;
        self.time = get_time_raw() + repeat.as_raw();
        Some(self)
    }

    pub fn get_time(&self) -> u64 {
        self.time
    }
}

impl Drop for TimerEntry {
    fn drop(&mut self) {
        if !self.args.is_null() {
            drop(unsafe { Box::from_raw(self.args as *mut u8) })
        }
    }
}

pub static TIMEBASE_FREQUENCY: spin::RwLock<Option<u32>> = spin::RwLock::new(None);

pub fn get_time() -> Time {
    Time::new(get_time_raw(), TimeUnit::Raw)
}

#[inline]
pub fn get_time_raw() -> u64 {
    let time: u64;
    unsafe {
        asm!("rdtime {}", out(reg) time);
    }
    time
}

#[inline]
pub fn next_tick() -> u64 {
    get_time_raw() + unsafe { TICK_CYCLE }
}

#[inline]
pub fn offset_sec(sec: u64) -> u64 {
    get_time_raw() + get_sec() * sec
}

pub fn get_sec() -> u64 {
    match &*TIMEBASE_FREQUENCY.read() {
        Some(t) => (*t) as u64,
        None => unimplemented!(),
    }
}

pub fn support_sstc() -> bool {
    unsafe { SSTC }
}

pub fn set_timer(time: &Time) {
    set_timer_raw(time.as_raw());
}

pub fn set_timer_raw(time: u64) {
    if support_sstc() {
        unsafe {
            asm!("csrw stimecmp, {}", in(reg) time);
        }
    } else {
        let _ = sbi::set_timer(time);
    }
}

pub fn add_timer<T>(delay: Time, callback: fn(*const u8), args: Option<Box<T>>, is_repeat: bool) {
    let time_container = &raw mut TIMER_QUEUE;
    let Some(timer_queue) = (unsafe { &mut *time_container }) else {
        return;
    };

    timer_queue.push(core::cmp::Reverse(TimerEntry::new(
        callback,
        if let Some(args) = args {
            Box::into_raw(args) as *const u8
        } else {
            ptr::null()
        },
        delay,
        is_repeat,
    )));
    let front = &timer_queue.peek().unwrap().0;
    set_timer_raw(front.time);
}

pub fn add_timer_c(delay: u64, callback: fn(*const u8), args: *const u8, is_repeat: bool) {
    let time_container = &raw mut TIMER_QUEUE;
    let Some(timer_queue) = (unsafe { &mut *time_container }) else {
        return;
    };
    let delay = Time::new(delay, TimeUnit::Sec);

    timer_queue.push(core::cmp::Reverse(TimerEntry::new(
        callback, args, delay, is_repeat,
    )));
    let front = &timer_queue.peek().unwrap().0;
    set_timer_raw(front.time);
}
