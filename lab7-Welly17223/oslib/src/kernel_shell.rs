extern crate alloc;
use alloc::{boxed::Box, string::String, vec::Vec};

use core::fmt::Write;

use crate::{
    fdt::{self, DTB_ADDR},
    file_system::{self, OpenFlags, VfsError, VnodeType},
    interrupt::{
        self,
        timer::{self, get_time_raw},
    },
    sbi,
    schedule::{self, current_tcb},
    thread::{self, ThreadControlTable},
    uart::{self, SERIAL},
};

struct TimerArgs {
    sec: u32,
    message: String,
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

pub fn control_input() {
    loop {
        let serial_ptr = &raw mut SERIAL;
        let Some(serial) = (unsafe { &mut *serial_ptr }) else {
            panic!("not initilized");
        };

        let mut buf = Vec::new();
        write!(serial, "> ").unwrap();
        loop {
            let ch = serial.pop_rx();

            match ch {
                b'\r' | b'\n' => {
                    serial.push_tx("\n");
                    break;
                }
                0x7f | 0x08 => {
                    if buf.pop().is_some() {
                        serial.push_tx("\x08 \x08");
                    }
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
                continue;
            }
        };
        let cmds: alloc::vec::Vec<_> = buf.split_ascii_whitespace().collect();
        let n_args = cmds.len();
        if cmds.is_empty() {
            continue;
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
            // "exit" => break 'main_loop,
            "ls" => {
                let pwd = &current_tcb().cwd;
                if let Some(item) = &pwd.item {
                    let vec = match item.list() {
                        Ok(v) => v,
                        Err(e) => {
                            writeln!(serial, "Error occur: {e:?}").unwrap();
                            continue;
                        }
                    };

                    vec.iter()
                        .for_each(|(name, _)| write!(serial, " {name}").unwrap());
                    writeln!(serial).unwrap();
                } else {
                    writeln!(serial, "Not found").unwrap();
                }
            }
            "cd" => {
                if n_args != 2 {
                    writeln!(serial, "usage: {} path", cmds[0]).unwrap();
                    continue;
                }

                let vfs = file_system::ROOT.get().unwrap();
                match vfs.lookup(cmds[1]) {
                    Ok(node) => {
                        if node.metadata.types != VnodeType::Directory {
                            writeln!(serial, "Error: {:?}", VfsError::NotADirectory).unwrap();
                            continue;
                        } else {
                            current_tcb().cwd = node;
                        }
                    }
                    Err(e) => writeln!(serial, "Error: {e:?}").unwrap(),
                };
            }
            "mkdir" => {
                if cmds.len() < 2 {
                    writeln!(serial, "usage: {} path", cmds[0]).unwrap();
                    continue;
                }

                let vfs = file_system::ROOT.get().unwrap();
                if let Err(e) = vfs.mkdir(cmds[1], true) {
                    writeln!(serial, "Err: {e:?}").unwrap();
                }
            }
            "dump" => {
                let dtb_addr = unsafe { DTB_ADDR } as *const u8;
                writeln!(serial).unwrap();
                fdt::dump_tree(dtb_addr);
            }
            "cat" if n_args > 1 => {
                let vfs = file_system::ROOT.get().unwrap();
                match vfs.open(
                    cmds[1],
                    OpenFlags {
                        read: true,
                        write: false,
                        create: false,
                    },
                ) {
                    Ok(mut f) => {
                        let mut buf = [0u8; 128];
                        while let Ok(n) = f.read(&mut buf)
                            && n > 0
                        {
                            match str::from_utf8(&buf[..n]) {
                                Ok(s) => {
                                    write!(serial, "{s}").unwrap();
                                }
                                Err(e) => {
                                    writeln!(serial, "utf8 parse error: {e}").unwrap();
                                    break;
                                }
                            }
                        }
                        writeln!(serial).unwrap();
                    }
                    Err(e) => {
                        writeln!(serial, "open file '{}' error {:?}", cmds[1], e).unwrap();
                    }
                }
            }
            "addtask" => {
                if cmds.len() < 3 {
                    writeln!(serial, "usage: {} [num] [priority]", cmds[0]).unwrap();
                    continue;
                }

                let num: u32 = match cmds[1].parse() {
                    Ok(n) => n,
                    Err(e) => {
                        writeln!(serial, "parse num error: {e:?}").unwrap();
                        continue;
                    }
                };

                let priority: u32 = match cmds[2].parse() {
                    Ok(n) => n,
                    Err(e) => {
                        writeln!(serial, "parse num error: {e:?}").unwrap();
                        continue;
                    }
                };

                interrupt::add_task(task_func, Box::new(num), priority);
            }
            "curr" => {
                let curr_task_ptr = &raw const interrupt::CURRENT_TASK;
                let queue_ptr = &raw const interrupt::TASK_QUEUE;
                let Some(curr_task) = (unsafe { &*curr_task_ptr }) else {
                    continue;
                };
                let Some(queue) = (unsafe { &*queue_ptr }) else {
                    continue;
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
                if cmds.len() < 2 {
                    writeln!(serial, "usage: {} [file name]", cmds[0]).unwrap();
                    continue;
                }
                let vfs = file_system::ROOT.get().unwrap();

                if let Ok(file) = vfs.open(cmds[1], file_system::OpenFlags::from("r")) {
                    let _disable_interrupt = interrupt::SModeInterrupt::new();

                    let current_pid = current_tcb().pid;
                    let proc =
                        thread::ThreadControlTable::new_user_thread(file, 0 as _, current_pid);
                    let pid = ThreadControlTable::create_thread(proc);

                    let exit_code = schedule::kwait_pid(pid);
                    writeln!(serial, "pid: {pid} exit code: {exit_code}").unwrap();
                }
            }
            "sstate" => {
                writeln!(serial, "sstate: {}", interrupt::s_mode_interrupt_status()).unwrap();
            }
            "settimer" => {
                if cmds.len() < 4 {
                    writeln!(serial, "usage: {} [sec] [is_repeat(0/1)] [msg]", cmds[0]).unwrap();
                    continue;
                }

                let sec: u64 = match cmds[1].parse() {
                    Ok(n) => n,
                    Err(e) => {
                        writeln!(serial, "parse sec error: {:?}", e).unwrap();
                        continue;
                    }
                };
                let is_repeat: bool = match cmds[2].parse::<u8>() {
                    Ok(1u8) => true,
                    Ok(0u8) => false,
                    Ok(n) => {
                        writeln!(serial, "only accept 0 and 1, found {n}").unwrap();
                        continue;
                    }
                    Err(e) => {
                        writeln!(serial, "parse repeat error: {:?}", e).unwrap();
                        continue;
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
                    continue;
                }

                let sec: u64 = match cmds[1].parse() {
                    Ok(n) => n,
                    Err(e) => {
                        writeln!(serial, "parse sec error: {:?}", e).unwrap();
                        continue;
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
}
