#![no_std]
#![no_main]

use core::{arch::global_asm, panic::PanicInfo};

mod platform;
mod sbi;
mod uart;

global_asm!(include_str!("start.s"));

#[cfg(all(feature = "qemu", feature = "orangePI"))]
compile_error!(
    "Features `qemu` and `orangePI` are mutually exclusive and cannot be enabled together."
);

#[cfg(not(any(feature = "qemu", feature = "orangePI")))]
compile_error!("You must specify a target platform feature: `qemu` or `orangePI`.");

fn new_command() {
    uart::puts("\n> ");
}

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
pub extern "C" fn main() {
    uart::puts("112550172's kernel starting...\n");

    let mut buf = [0u8; 32];
    let mut is_buf_full = false;
    'main_loop: loop {
        new_command();
        let mut curr_str = 0;

        'l: loop {
            let ch = uart::getc();

            match ch as u8 {
                b'\n' => break 'l,
                // backspace
                0x7f | 0x08 => {
                    if is_buf_full {
                        is_buf_full = false;
                        uart::puts(Key::Down.to_str());
                        uart::puts(Key::Clear.to_str());
                        uart::puts(Key::Up.to_str());
                    }
                    if curr_str > 0 {
                        uart::puts("\x08 \x08");
                        buf[curr_str] = 0;
                        curr_str -= 1;
                    }
                }
                0x04 => break 'main_loop,
                _ => {
                    if curr_str != buf.len() - 1 {
                        buf[curr_str] = ch as u8;
                        uart::putc(ch as u8);
                        curr_str += 1;
                    } else if !is_buf_full {
                        uart::puts("\n\r");
                        uart::puts(Key::Clear.to_str());
                        uart::puts("[warn]: buffer is full");
                        uart::puts(Key::Up.to_str());
                        uart::puts(Key::Clear.to_str());
                        uart::puts("\r> ");
                        uart::puts(str::from_utf8(&buf).unwrap());

                        is_buf_full = true;
                    }
                }
            }
        }

        let cmd = str::from_utf8(&buf[..curr_str]).unwrap();
        match cmd {
            "help" => {
                uart::puts("\nAvaliable commands:");
                uart::puts("\n  help  - print help message.");
                uart::puts("\n  hello - print Hello world.");
                uart::puts("\n  info  - print system info.");
                uart::puts("\n  exit  - leave.");
            }
            "hello" => {
                uart::puts("\nHello world!");
            }
            "info" => {
                uart::puts("\nSystem information:\n");
                uart::puts("  OpenSBI specification version: ");
                uart::put_hex(sbi::get_spec_version());
                uart::putc(b'\n');

                uart::puts("  Implementation ID: ");
                uart::put_hex(sbi::get_impl_id());
                uart::putc(b'\n');

                uart::puts("  Implementation version: ");
                uart::put_hex(sbi::get_impl_version());
            }
            "exit" => break 'main_loop,
            _ => {
                uart::puts("\nInvalid command '");
                uart::puts(cmd);
                uart::putc(b'\'');
            }
        }
    }

    uart::puts("\nBye!\n");

    sbi::rust_sbi_ecall(0x53525354, 0x0, 0, 0, 0, 0, 0, 0).unwrap();
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    uart::puts("Something went wrong!\n");
    loop {}
}
