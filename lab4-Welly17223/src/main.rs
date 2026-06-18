#![no_std]
#![no_main]
extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use core::arch::asm;
use core::fmt::Write;
use core::{arch::global_asm, panic::PanicInfo};
use core::{ptr, str};

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use log::{info, trace, warn};
use memory_alloc::ALLOCATOR;
use oslib::interrupt::timer::{TimeUnit, get_time_raw};
use oslib::interrupt::{self, plic, timer};
use oslib::ramdisk::{CatError, Cpio};
use oslib::uart::{self, SERIAL, Uart};
use oslib::{fdt, logger, platform, ramdisk, sbi, schedule};
use riscv::register::sstatus::SPP;

global_asm!(include_str!("start.s"));

#[cfg(all(feature = "qemu", feature = "orangePI"))]
compile_error!(
    "Features `qemu` and `orangePI` are mutually exclusive and cannot be enabled together."
);

#[cfg(not(any(feature = "qemu", feature = "orangePI")))]
compile_error!("You must specify a target platform feature: `qemu` or `orangePI`.");

enum Key {
    Up,
    Down,
    Clear,
}

impl Key {
    pub fn to_str(&self) -> &'static str {
        match self {
            Self::Up => "\x1b[A",
            Self::Down => "\x1b[B",
            Self::Clear => "\x1b[2K",
        }
    }
}

static mut DTB_ADDR: usize = 0;
static mut INITRD_START: usize = 0;

struct TimerArgs {
    sec: u32,
    message: String,
}

fn boot_func(_args: *const u8) {
    let serial_ptr = &raw mut SERIAL;
    let Some(serial) = (unsafe { &mut *serial_ptr }) else {
        return;
    };
    writeln!(
        serial,
        "boot time: {} sec",
        timer::get_time_raw() / timer::get_sec()
    )
    .unwrap();
}

fn timer_func(args: *const u8) {
    let args = unsafe { &*(args as *const TimerArgs) };
    let _disable_interrupt = interrupt::SModeInterrupt::new();
    let serial_ptr = &raw mut SERIAL;
    let Some(serial) = (unsafe { &mut *serial_ptr }) else {
        return;
    };

    writeln!(
        serial,
        "[time interrupt]: {} call at {} message: {}",
        timer::get_time_raw() / timer::get_sec(),
        args.sec,
        args.message
    )
    .unwrap();
}

extern "C" fn task_func(args: *const u8) {
    let serial_ptr = &raw mut uart::SERIAL;
    let Some(serial) = (unsafe { &mut *serial_ptr }) else {
        return;
    };
    let args: u32 = unsafe { *(args as *const u32) };
    let next_time = timer::offset_sec(args as u64);
    while timer::get_time_raw() < next_time {}

    let _disable_interrupt = interrupt::SModeInterrupt::default();
    writeln!(serial, "[task] run {} loops", args).unwrap();
}

fn control_input(_args: *const u8) {
    let serial_ptr = &raw mut SERIAL;
    let Some(serial) = (unsafe { &mut *serial_ptr }) else {
        return;
    };
    let linux_initrd_start = unsafe { INITRD_START } as *const Cpio;

    let mut buf = Vec::new();
    write!(serial, "> ").unwrap();
    loop {
        let Some(ch) = serial.pop_rx_ch() else {
            continue;
        };

        match ch {
            b'\r' | b'\n' => {
                serial.push_tx_ch(b'\n');
                break;
            }
            0x7f | 0x08 => {
                buf.pop();
                serial.push_tx(b"\x08 \x08");
            }
            _ => {
                buf.push(ch);
                write!(serial, "{}", ch as char).unwrap();
            }
        }
    }

    let buf = match String::from_utf8(buf) {
        Ok(s) => s,
        Err(e) => {
            writeln!(serial, "{e:?}").unwrap();
            return;
        }
    };
    let cmds: alloc::vec::Vec<_> = buf.split_ascii_whitespace().collect();
    let n_args = cmds.len();
    if cmds.is_empty() {
        return;
    }

    let _disable_interrupt = interrupt::SModeInterrupt::new();
    match cmds[0] {
        "help" => {
            writeln!(serial, "Avaliable commands:").unwrap();
            writeln!(serial, "    {:8} - print help message.", "help").unwrap();
            writeln!(serial, "    {:8} - print Hello world.", "hello").unwrap();
            writeln!(serial, "    {:8} - print system info.", "info").unwrap();
            writeln!(serial, "    {:8} - list file in file system.", "ls").unwrap();
            writeln!(serial, "    {:8} - cat file in file system.", "cat").unwrap();
            writeln!(
                serial,
                "    {:8} - set oneshot timer, and print message.",
                "settimer"
            )
            .unwrap();
        }
        "hello" => {
            writeln!(serial, "Hello world!").unwrap();
        }
        "info" => {
            writeln!(serial, "System information:").unwrap();
            writeln!(
                serial,
                "  OpenSBI specification version: 0x{:x}",
                sbi::get_spec_version()
            )
            .unwrap();
            writeln!(serial, "  Implementation ID: 0x{:x}", sbi::get_impl_id()).unwrap();
            writeln!(
                serial,
                "  Implementation version: 0x{:x}",
                sbi::get_impl_version()
            )
            .unwrap();
        }
        "memory" => {
            writeln!(serial, "{}", ALLOCATOR).unwrap();
        }
        // "exit" => break 'main_loop,
        "malloc" => {
            if n_args != 2 {
                writeln!(serial, "usage: {} [size]", cmds[0]).unwrap();
                return;
            }
            let size: usize = match if cmds[1].starts_with("0x") {
                usize::from_str_radix(cmds[1].strip_prefix("0x").unwrap(), 16)
            } else {
                cmds[1].parse()
            } {
                Ok(h) => h,
                Err(e) => {
                    writeln!(serial, "Parse {} fail: {:?}", cmds[1], e).unwrap();
                    writeln!(serial, "usage: {} [ptr]", cmds[0]).unwrap();
                    return;
                }
            };
            let size_zero = usize::BITS - size.leading_zeros();
            let align = 1 << (size_zero);

            let ptr = unsafe { ALLOCATOR.alloc(Layout::from_size_align_unchecked(size, align)) };

            match ptr.is_null() {
                false => writeln!(serial, "Alloc at 0x{:x}", ptr as usize),
                true => writeln!(serial, "Alloc return None!"),
            }
            .unwrap()
        }
        "free" => {
            if n_args != 2 {
                writeln!(serial, "usage: {} [ptr]", cmds[0]).unwrap();
                return;
            }
            let ptr: usize = match if cmds[1].starts_with("0x") {
                usize::from_str_radix(cmds[1].strip_prefix("0x").unwrap(), 16)
            } else {
                cmds[1].parse()
            } {
                Ok(h) => h,
                Err(e) => {
                    writeln!(serial, "Parse {} fail: {:?}", cmds[1], e).unwrap();
                    writeln!(serial, "usage: {} [ptr]", cmds[0]).unwrap();
                    return;
                }
            };

            writeln!(serial).unwrap();
            unsafe {
                ALLOCATOR.dealloc(ptr as *mut u8, Layout::new::<usize>());
            }
        }
        "ls" => {
            ramdisk::list(linux_initrd_start).unwrap();
        }
        "dump" => {
            let dtb_addr = unsafe { DTB_ADDR } as *const u8;
            writeln!(serial).unwrap();
            fdt::dump_tree(dtb_addr);
        }
        "cat" if n_args > 1 => {
            if let Err(CatError::FileNotFound) = ramdisk::cat(linux_initrd_start, cmds[1]) {
                writeln!(serial, "File '{}' not fmound", cmds[1]).unwrap();
            }
        }
        "addtask" => {
            if cmds.len() < 3 {
                writeln!(serial, "usage: {} [num] [priority]", cmds[0]).unwrap();
                return;
            }

            let num: u32 = match cmds[1].parse() {
                Ok(n) => n,
                Err(e) => {
                    writeln!(serial, "parse num error: {e:?}").unwrap();
                    return;
                }
            };

            let priority: u32 = match cmds[2].parse() {
                Ok(n) => n,
                Err(e) => {
                    writeln!(serial, "parse num error: {e:?}").unwrap();
                    return;
                }
            };

            interrupt::add_task(task_func, Box::new(num), priority);
        }
        "curr" => {
            let curr_task_ptr = &raw const interrupt::CURRENT_TASK;
            let queue_ptr = &raw const interrupt::TASK_QUEUE;
            let Some(curr_task) = (unsafe { &*curr_task_ptr }) else {
                return;
            };
            let Some(queue) = (unsafe { &*queue_ptr }) else {
                return;
            };
            let peek = queue.peek();
            writeln!(
                serial,
                "current task: id {}, priority {}",
                curr_task.id(),
                curr_task.priority(),
            )
            .unwrap();
            if let Some(peek) = peek {
                writeln!(
                    serial,
                    "peek task: id {}, priority {}",
                    peek.id(),
                    peek.priority(),
                )
                .unwrap();
            } else {
                writeln!(serial, "queue is empty").unwrap();
            }
        }
        "sepc" => {
            writeln!(serial, "sepc: {:#x}", riscv::register::sepc::read()).unwrap();
        }
        "exec" => {
            use alloc::vec;
            if cmds.len() < 2 {
                writeln!(serial, "usage: {} [file name]", cmds[0]).unwrap();
                return;
            }

            if let Ok(file) = ramdisk::find(linux_initrd_start, cmds[1]) {
                const STACK_DEPTH: usize = 0x4000;
                let mut program = vec![0u8; STACK_DEPTH];

                for (idx, byte) in file.iter().enumerate() {
                    program[idx] = *byte;
                }

                let program_start = program.as_ptr() as *mut u8;
                let mut prog_regs = interrupt::pt_regs::default();

                prog_regs.sepc = program_start as usize;
                // spie
                prog_regs.sstatus |= 1 << 5;
                // spp
                prog_regs.sstatus &= !(1 << 8);
                // stack
                prog_regs.sscratch = program_start.wrapping_add(STACK_DEPTH) as usize;
                writeln!(
                    serial,
                    "will exec: {} at {:#x}, {:#p}, stack: {:#x}",
                    cmds[1],
                    prog_regs.sepc,
                    program.as_ptr(),
                    prog_regs.sscratch
                )
                .unwrap();
                unsafe {
                    riscv::register::sepc::write(prog_regs.sepc);
                    riscv::register::sstatus::set_spp(SPP::User);
                    riscv::register::sstatus::set_spie();
                    riscv::register::sscratch::write(prog_regs.sscratch);
                    asm!("sret");
                }
            } else {
                writeln!(serial, "file {} not found", cmds[1]).unwrap();
            }
        }
        "sstate" => {
            writeln!(serial, "sstate: {}", interrupt::s_mode_interrupt_status()).unwrap();
        }
        "tasktest" => unsafe {
            c_test::test_addtask();
        },
        "settimer" => {
            if cmds.len() < 4 {
                writeln!(serial, "usage: {} [sec] [is_repeat(0/1)] [msg]", cmds[0]).unwrap();
                return;
            }

            let sec: u64 = match cmds[1].parse() {
                Ok(n) => n,
                Err(e) => {
                    writeln!(serial, "parse sec error: {:?}", e).unwrap();
                    return;
                }
            };
            let is_repeat: bool = match cmds[2].parse::<u8>() {
                Ok(1u8) => true,
                Ok(0u8) => false,
                Ok(n) => {
                    writeln!(serial, "only accept 0 and 1, found {n}").unwrap();
                    return;
                }
                Err(e) => {
                    writeln!(serial, "parse repeat error: {:?}", e).unwrap();
                    return;
                }
            };

            let message = cmds[3..].join(" ");
            let t1 = Box::new(TimerArgs {
                sec: (get_time_raw() / timer::get_sec()) as u32,
                message,
            });
            timer::add_timer(
                timer::Time::new(sec, timer::TimeUnit::Sec),
                timer_func,
                Some(t1),
                is_repeat,
            );
        }
        "setTimeout" => {
            if cmds.len() < 3 {
                writeln!(serial, "usage: {} [sec] [is_repeat(0/1)] [msg]", cmds[0]).unwrap();
                return;
            }

            let sec: u64 = match cmds[1].parse() {
                Ok(n) => n,
                Err(e) => {
                    writeln!(serial, "parse sec error: {:?}", e).unwrap();
                    return;
                }
            };

            let message = cmds[2..].join(" ");
            let t1 = Box::new(TimerArgs {
                sec: (get_time_raw() / timer::get_sec()) as u32,
                message,
            });
            timer::add_timer(
                timer::Time::new(sec, timer::TimeUnit::Sec),
                timer_func,
                Some(t1),
                false,
            );
        }
        "time" => {
            writeln!(
                serial,
                "current time: {}, freq: {}, timmer interrupt: {}, timmer sip: {}",
                timer::get_time_raw(),
                timer::get_sec(),
                riscv::register::sie::read().stimer(),
                riscv::register::sip::read().stimer()
            )
            .unwrap();
        }
        _ => {
            writeln!(serial, "Invalid command '{}'", cmds[0]).unwrap();
        }
    }
}

#[allow(static_mut_refs)]
#[unsafe(no_mangle)]
pub extern "C" fn main(hart_id: u64, dtb_addr: u64) {
    let dtb_addr = dtb_addr as *const u8;

    // Init dynamic memory
    let kernel_end: usize;
    let kernel_start: usize;
    let stack_buttom: usize;
    unsafe {
        asm!("la {}, _start", out(reg) kernel_start);
        asm!("la {}, __stack_top", out(reg) kernel_end);
        asm!("la {}, __stack_buttom", out(reg) stack_buttom);
    }
    let memory_alloc = match memory_alloc::init_memory_allocator(
        dtb_addr,
        kernel_start as *mut u8,
        kernel_end as *mut u8,
    ) {
        Ok(m) => m,
        Err(e) => {
            log::error!("init memory: {:#?}", e);
            panic!("{:#?}", e);
        }
    };
    ALLOCATOR.init(memory_alloc);

    // Init interrupt
    match interrupt::interrupt_init(dtb_addr as *mut u8, hart_id as usize) {
        Ok(_) => (),
        Err(_) => {
            panic!("plic error");
        }
    };

    // init default serial uart
    uart::init_serial(dtb_addr);

    // init logginer
    log::set_logger(&logger::LOGGER).unwrap();
    log::set_max_level(log::LevelFilter::Warn);

    if let Some(serial) = unsafe { &mut SERIAL } {
        serial.getc();
    }

    // Test logger
    trace!("Test trace");
    log::debug!("Test debug");
    info!("Test info");
    warn!("Test warn");
    log::error!("Test error");

    info!("Stack buttom: {:#x}", stack_buttom);
    info!("start at hart: {hart_id}");
    info!(
        "frequency: {}, curr_time: {}",
        timer::get_sec(),
        timer::get_time_raw()
    );
    info!("memory: {}", ALLOCATOR);

    // set uart to async
    if let Some(uart) = unsafe { &mut SERIAL } {
        uart.set_interrupt(hart_id as usize);
    }

    let offset_chosen = match fdt::path_offset(dtb_addr, "/chosen", 1) {
        Ok(o) => o,
        Err(_) => return,
    };
    let linux_initrd_start = match fdt::getprop(dtb_addr, offset_chosen, "linux,initrd-start") {
        Ok(p) => {
            unsafe { *(p.0 as *const u32).wrapping_byte_offset(4) }.swap_bytes() as *const Cpio
        }
        Err(_) => return,
    };
    unsafe {
        DTB_ADDR = dtb_addr as usize;
        INITRD_START = linux_initrd_start as usize;
    }

    timer::add_timer::<u8>(timer::Time::new(2, TimeUnit::Sec), boot_func, None, true);
    timer::add_timer::<u8>(
        timer::Time::new(unsafe { timer::TICK_CYCLE }, timer::TimeUnit::Raw),
        c_test::test_func_wrapper,
        None,
        false,
    );
    interrupt::s_mode_interrupt_enable();

    loop {
        control_input(ptr::null());
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let mut def_uart = Uart::new(platform::UART_BASE as usize, platform::UART_REG_SHIFT);
    def_uart.puts("Something went wrong!\n");

    if let Some(message) = info.message().as_str() {
        def_uart.puts(message);
        def_uart.puts("\n");
    } else {
        def_uart.puts("No panic message\n");
    }

    if let Some(locatioin) = info.location() {
        def_uart.puts(locatioin.file());
        def_uart.puts(" ");
        def_uart.put_dec(locatioin.line() as u64);
        def_uart.puts("\n");
    } else {
        def_uart.puts("No location message\n");
    }
    loop {
        unsafe { asm!("wfi") };
    }
}
