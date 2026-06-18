#![no_std]
#![no_main]

use core::fmt::Write;
use core::{arch::global_asm, panic::PanicInfo};
use oslib::ramdisk::{CatError, Cpio};
use oslib::uart::Uart;
use oslib::{fdt, platform, ramdisk, sbi};

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

#[allow(static_mut_refs)]
#[unsafe(no_mangle)]
pub extern "C" fn main(_heart_id: u64, dtb_addr: u64) {
    let dtb_addr = dtb_addr as *const u8;
    let offset_soc_serial = match fdt::path_offset(dtb_addr, "/soc/serial", 1) {
        Ok(o) => o,
        Err(_) => return,
    };
    let offset_chosen = match fdt::path_offset(dtb_addr, "/chosen", 1) {
        Ok(o) => o,
        Err(_) => return,
    };

    let (reg_ptr, _) = match fdt::getprop(dtb_addr, offset_soc_serial, "reg") {
        Ok(prop) => (prop.0 as *mut u32, prop.1),
        Err(_) => return,
    };

    let mut def_uart = match unsafe { *reg_ptr.wrapping_offset(1) }.swap_bytes() {
        // Qemu
        v if v == 0x10000000 => Uart::new(v as usize, (v + 0x5) as usize),
        // Other
        v => Uart::new(v as usize, (v + 0x14) as usize),
    };
    def_uart.getc();

    writeln!(def_uart, "{}'s kernel starting...", 112550172).unwrap();
    writeln!(def_uart, "Dtb Address: 0x{:x}", dtb_addr as u32).unwrap();

    let linux_initrd_start = match fdt::getprop(dtb_addr, offset_chosen, "linux,initrd-start") {
        Ok(p) => {
            unsafe { *(p.0 as *const u32).wrapping_byte_offset(4) }.swap_bytes() as *const Cpio
        }
        Err(_) => return,
    };
    let linux_initrd_end = match fdt::getprop(dtb_addr, offset_chosen, "linux,initrd-end") {
        Ok(p) => unsafe { *(p.0 as *const u32).wrapping_byte_offset(4) }.swap_bytes(),
        Err(_) => return,
    };

    write!(
        def_uart,
        "linux_initrd: 0x{:8x} 0x{:8x}",
        linux_initrd_start as u32, linux_initrd_end
    )
    .unwrap();

    let mut buf = [0u8; 32];
    let mut is_buf_full = false;
    'main_loop: loop {
        def_uart.puts("\n> ");
        let mut curr_str = 0;

        'l: loop {
            let ch = def_uart.getc();

            match ch as u8 {
                b'\n' => break 'l,
                // backspace
                0x7f | 0x08 => {
                    if is_buf_full {
                        is_buf_full = false;
                        def_uart.puts(Key::Down.to_str());
                        def_uart.puts(Key::Clear.to_str());
                        def_uart.puts(Key::Up.to_str());
                    }
                    if curr_str > 0 {
                        def_uart.puts("\x08 \x08");
                        buf[curr_str] = 0;
                        curr_str -= 1;
                    }
                }
                0x04 => break 'main_loop,
                _ => {
                    if curr_str != buf.len() - 1 {
                        buf[curr_str] = ch as u8;
                        def_uart.putc(ch as u8);
                        curr_str += 1;
                    } else if !is_buf_full {
                        def_uart.puts("\n\r");
                        def_uart.puts(Key::Clear.to_str());
                        def_uart.puts("[warn]: buffer is full");
                        def_uart.puts(Key::Up.to_str());
                        def_uart.puts(Key::Clear.to_str());
                        def_uart.puts("\r> ");
                        def_uart.puts(str::from_utf8(&buf).unwrap());

                        is_buf_full = true;
                    }
                }
            }
        }

        let mut cmds: [&str; 10] = [""; 10];
        let mut n_args = 0usize;
        for (i, cmd) in str::from_utf8(&buf[..curr_str])
            .unwrap()
            .split_ascii_whitespace()
            .take(10)
            .enumerate()
        {
            cmds[i] = cmd;
            n_args += 1;
        }

        match cmds[0] {
            "help" => {
                def_uart.puts("\nAvaliable commands:");
                def_uart.puts("\n  help  - print help message.");
                def_uart.puts("\n  hello - print Hello world.");
                def_uart.puts("\n  info  - print system info.");
                def_uart.puts("\n  ls    - list file in file system.");
                def_uart.puts("\n  cat   - cat file in file system.");
                def_uart.puts("\n  exit  - leave.");
            }
            "hello" => {
                def_uart.puts("\nHello world!");
            }
            "info" => {
                def_uart.puts("\nSystem information:\n");
                writeln!(
                    def_uart,
                    "  OpenSBI specification version: 0x{:x}",
                    sbi::get_spec_version()
                )
                .unwrap();
                writeln!(def_uart, "  Implementation ID: 0x{:x}", sbi::get_impl_id()).unwrap();
                write!(
                    def_uart,
                    "  Implementation version: 0x{:x}",
                    sbi::get_impl_version()
                )
                .unwrap();
            }
            "exit" => break 'main_loop,
            "ls" => {
                def_uart.puts("\n");
                ramdisk::list(&def_uart, linux_initrd_start).unwrap();
            }
            "dump" => {
                def_uart.puts("\n");
                fdt::dump_tree(&def_uart, dtb_addr);
            }
            "cat" if n_args > 1 => {
                def_uart.puts("\n");
                if let Err(CatError::FileNotFound) =
                    ramdisk::cat(&def_uart, linux_initrd_start, cmds[1])
                {
                    write!(def_uart, "File '{}' not fmound", cmds[1]).unwrap();
                }
            }
            _ => {
                write!(def_uart, "\nInvalid command '{}'", cmds[0]).unwrap();
            }
        }
    }

    def_uart.puts("\nBye!\n");

    // sbi::rust_sbi_ecall(0x53525354, 0x0, 0, 0, 0, 0, 0, 0).unwrap();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let def_uart = Uart::new(platform::UART_BASE as usize, platform::UART_LSR as usize);
    def_uart.puts("Something went wrong!\n");

    if let Some(message) = info.message().as_str() {
        def_uart.puts(message);
    } else {
        def_uart.puts("No error message\n");
    }

    if let Some(locatioin) = info.location() {
        def_uart.puts(locatioin.file());
        def_uart.puts(" ");
        def_uart.put_hex(locatioin.line() as u64);
        def_uart.puts("\n");
    } else {
        def_uart.puts("No location message\n");
    }
    loop {}
}
