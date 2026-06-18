#![no_std]
// #![cfg_attr(not(test), no_std)]
#![no_main]

use core::ffi::c_ulong;
use core::{arch::asm, fmt::Write, panic::PanicInfo};
use core::{mem, ptr};
use crate::uart::Uart;

mod uart;
mod platform;
mod fdt;

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
// const KERNEL_BASE_ADDRESS: usize = 0x82000000;
const START_TOKEN: u32 = 0x544F4F42;

/* #[unsafe(no_mangle)]
extern "C" fn relocation(hart_id: usize, dtb_address: usize) -> ! {
    let dtb_addr = dtb_address as *const u8;

    let offset_soc_serial = fdt::path_offset(dtb_addr, "/soc/serial", 1).unwrap();
    let (reg_ptr, _) = fdt::getprop(dtb_addr, offset_soc_serial, "reg").unwrap();
    let reg_ptr = reg_ptr as *mut u32;
    let uart_shift = match fdt::getprop(dtb_addr, offset_soc_serial, "reg-shift") {
        Ok((ptr, _)) => unsafe { *(ptr as *const u32) }.swap_bytes(),
        Err(fdt::Error::Notfound) => 0,
        Err(_) => panic!("Error find reg-shift"),
    };
    let mut def_uart = match unsafe { *reg_ptr.wrapping_offset(1) }.swap_bytes() {
        v if v == 0x10000000 => Uart::new(v as usize, uart_shift),
        v => Uart::new(v as usize, uart_shift),
    };

    let mut mem_offset = [0usize; 32];
    let mem_node_counts = fdt::path_all_offset(dtb_addr, "/memory", &mut mem_offset).unwrap();
    let (target_addr, _) = mem_offset[..mem_node_counts].iter().fold(
        (0usize, 0usize),
        |(mut prev_size, mut prev_addr), off| {
            let (ptr, len) = fdt::getprop(dtb_addr, *off, "reg").unwrap();
            let ptr = ptr as *mut u32;

            let entry_num = (len / size_of::<u64>() / 2) as isize;
            for i in 0..entry_num {
                let curr_ind = i << 1;
                let base_addr = ((unsafe { *ptr.wrapping_offset(curr_ind) }.swap_bytes() as usize)
                    << 32)
                    | unsafe { *ptr.wrapping_offset(curr_ind + 1) }.swap_bytes() as usize;
                let size = ((unsafe { *ptr.wrapping_offset(curr_ind + 2) }.swap_bytes() as usize)
                    << 32)
                    | unsafe { *ptr.wrapping_offset(curr_ind + 3) }.swap_bytes() as usize;

                if size > prev_addr {
                    prev_addr = base_addr;
                    prev_size = size;
                }
            }

            (prev_addr, prev_size)
        },
    );

    writeln!(def_uart, "Find new address: {:x}", target_addr).unwrap();

    let _start_addr: usize;
    let _start_main_addr: usize;
    let _end_addr: usize;
    unsafe {
        asm!("la {}, _start", out(reg) _start_addr);
        asm!("la {}, _start_main", out(reg) _start_main_addr);
        asm!("la {}, _end", out(reg) _end_addr);
    }

    let target_addr = if target_addr == 0x80000000 {
        target_addr.wrapping_add(0x2000000)
    } else {
        target_addr
    };

    unsafe {
        ptr::copy_nonoverlapping(
            _start_addr as *const u8,
            target_addr as *mut u8,
            _end_addr - _start_addr,
        );
    }

    let offset = _start_main_addr - _start_addr;
    writeln!(
        def_uart,
        "Copy {} bytes from 0x{:x} to 0x{:x} done! jump to 0x{:x}",
        _end_addr - _start_addr,
        _start_addr,
        target_addr,
        target_addr + offset
    )
    .unwrap();

    let target_func: extern "C" fn(usize, usize) -> ! =
        unsafe { mem::transmute(target_addr + offset) };

    target_func(hart_id, dtb_addr as usize)
} */

#[unsafe(no_mangle)]
extern "C" fn main(hart_id: usize, dtb_address: usize) {
    // const KERNEL_BASE_ADDRESS: usize = 0x82000000;
    let kernel_base_address: usize = 0x20000000; // 0x02000000;

    let dtb_addr = dtb_address as *const u8;
    let offset_soc_serial = match fdt::path_offset(dtb_addr, "/soc/serial", 1) {
        Ok(o) => o,
        Err(_) => return,
    };
    let (reg_ptr, _) = match fdt::getprop(dtb_addr, offset_soc_serial, "reg") {
        Ok(prop) => (prop.0 as *mut u32, prop.1),
        Err(_) => return,
    };

    let uart_shift = match fdt::getprop(dtb_addr, offset_soc_serial, "reg-shift") {
        Ok((ptr, _)) => unsafe { *(ptr as *const u32) }.swap_bytes(),
        Err(fdt::Error::Notfound) => 0,
        Err(_) => panic!("Error find reg-shift"),
    };
    let mut def_uart = match unsafe { *reg_ptr.wrapping_offset(1) }.swap_bytes() {
        v if v == 0x10000000 => Uart::new(v as usize, uart_shift),
        v => Uart::new(v as usize, uart_shift),
    };

    let mut mem_offset = [0usize; 32];
    let mem_node_counts = if let Ok(c) = fdt::path_all_offset(dtb_addr, "/memory", &mut mem_offset)
    {
        c
    } else {
        return;
    };

    let mut reserve_mem_offset = [0usize; 32];
    let reserve_mem_counts =
        match fdt::path_all_offset(dtb_addr, "/reserved-memory", &mut reserve_mem_offset) {
            Ok(e) => e,
            Err(fdt::Error::Notfound) => {
                writeln!(def_uart, "Not found\n").unwrap();
                0
            }
            Err(_) => panic!("Reserved wrong"),
        };

    writeln!(def_uart, "This is bootloader").unwrap();
    writeln!(
        def_uart,
        "Uart addr: {:x}",
        unsafe { *reg_ptr.wrapping_offset(1) }.swap_bytes()
    )
    .unwrap();
    writeln!(def_uart, "Find {} memory nodes", mem_node_counts).unwrap();

    for o in &mem_offset[..mem_node_counts] {
        let (ptr, len) = fdt::getprop(dtb_addr, *o, "reg").unwrap();
        let ptr = ptr as *mut u32;

        for i in 0..(len / size_of::<u32>() / 2) as isize {
            write!(
                def_uart,
                "0x{:x}",
                ((unsafe { *ptr.wrapping_offset(i * 2) }.swap_bytes() as usize) << 32)
                    | unsafe { *ptr.wrapping_offset(i * 2 + 1) }.swap_bytes() as usize
            )
            .unwrap();
            def_uart.puts(" ");
        }
        def_uart.puts("\n");
    }

    loop {
        let mut input_buf = [0u8; 32];
        let mut input_offset = 0usize;
        let mut is_buf_full = false;
        def_uart.puts("\n(Bootloader)> ");
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
                    if input_offset > 0 {
                        def_uart.puts("\x08 \x08");
                        input_buf[input_offset] = 0;
                        input_offset -= 1;
                    }
                }
                _ => {
                    if input_offset != input_buf.len() - 1 {
                        input_buf[input_offset] = ch as u8;
                        def_uart.putc(ch as u8);
                        input_offset += 1;
                    } else if !is_buf_full {
                        def_uart.puts("\n\r");
                        def_uart.puts(Key::Clear.to_str());
                        def_uart.puts("[warn]: buffer is full");
                        def_uart.puts(Key::Up.to_str());
                        def_uart.puts(Key::Clear.to_str());
                        def_uart.puts("\r(Bootloader)> ");
                        def_uart.puts(input_buf);

                        is_buf_full = true;
                    }
                }
            }
        }
        let cmd = if let Ok(cmd) = str::from_utf8(&input_buf[..input_offset]) {
            cmd
        } else {
            def_uart.puts("\nYour input include 'non-ascii' words!");
            continue;
        };

        match cmd {
            "load" => {
                def_uart.puts("\nwaiting for magic number ");
                def_uart.put_hex(START_TOKEN as u64);
                def_uart.putc(b'\n');

                let start_token = def_uart.get_u32();
                def_uart.put_hex(start_token as u64);
                if start_token != START_TOKEN {
                    def_uart.puts("\nInvalid start token, stop transimission");
                    continue;
                }

                def_uart.puts("\nGet magic number! waiting for kernel transition...");

                // byte size
                let kernel_size = def_uart.get_u32() as usize;
                def_uart.put_hex(kernel_size as u32);

                let begin_signal: u32 = 0x123fd8ae;
                let mut recv_byte = 0usize;
                let block_size = 1024usize;

                def_uart.puts("start\n");
                let mut my_sig = 0u32;
                while recv_byte < kernel_size {
                    while my_sig != begin_signal {
                        let ch = def_uart.get_raw_byte() as u8;
                        my_sig <<= 8;
                        my_sig += ch as u32;
                    }
                    def_uart.puts("start\n");

                    let wanted_byte = if block_size > kernel_size - recv_byte {
                        kernel_size - recv_byte
                    } else {
                        block_size
                    };

                    let mut temp_recv_byte = 0usize;
                    for i in 0..wanted_byte {
                        let instruct = def_uart.get_raw_byte() as u8;
                        let curr_addr = (kernel_base_address + recv_byte + i) as *mut u8;
                        my_sig <<= 8;
                        my_sig += instruct as u32;

                        unsafe {
                            *curr_addr = instruct;
                        }
                        temp_recv_byte += 1;

                        if my_sig == begin_signal {
                            def_uart.puts("NAK\n");
                            break;
                        }
                    }

                    if temp_recv_byte == wanted_byte {
                        recv_byte += temp_recv_byte;
                        def_uart.puts("ACK\n");
                        my_sig = 0;
                    }
                }

                def_uart.puts("\nfile size: ");
                def_uart.put_dec(kernel_size as u64);
                def_uart.puts(" ");
                def_uart.put_hex(kernel_size as u32);
                def_uart.puts("\nwrite from: ");
                def_uart.put_hex(kernel_base_address as u64);
                def_uart.puts("\nto: ");
                def_uart.put_hex((kernel_base_address + kernel_size) as u64);

                def_uart.puts("\nkernel transition done, jump to kernel\n");

                unsafe {
                    asm!("fence.i");
                }
                let kernel: extern "C" fn(c_ulong, c_ulong) =
                    unsafe { core::mem::transmute(kernel_base_address) };
                kernel(hart_id as u64, dtb_address as u64);

                def_uart.puts("\nkernel end, we are back!");
            }
            "help" => {
                def_uart.puts("\nThere only one command usable");
                def_uart.puts("\n  load  -  load kernel");
            }
            "pc" => {
                let pc: usize;
                unsafe {
                    asm!("auipc {}, 0", out(reg) pc);
                }
                write!(def_uart, "\npc: {:x}", pc).unwrap();
            }
            "memory" => {
                for o in &mem_offset[..mem_node_counts] {
                    let (ptr, len) = fdt::getprop(dtb_addr, *o, "reg").unwrap();
                    let ptr = ptr as *mut u32;

                    for i in 0..(len / size_of::<u64>()) as isize {
                        let ind = i << 1;
                        if i & (0x1) == 0 {
                            def_uart.putc(b'\n');
                        }
                        write!(
                            def_uart,
                            "0x{:x} ",
                            ((unsafe { *ptr.wrapping_offset(ind) }.swap_bytes() as u64) << 32)
                                | (unsafe { *ptr.wrapping_offset(ind + 1) }.swap_bytes() as u64)
                        )
                        .unwrap();
                    }
                }
            }
            "reserve" => {
                reserve_mem_offset[..reserve_mem_counts]
                    .iter()
                    .for_each(|off| {
                        let (ptr, len) = fdt::getprop(dtb_addr, *off, "reg").unwrap();
                        let ptr = ptr as *const u32;

                        for i in 0..(len / size_of::<u64>() / 2) {
                            let ind = i << 1;
                            let base_addr =
                                ((unsafe { *ptr.wrapping_add(ind) }.swap_bytes() as u64) << 32)
                                    | unsafe { *ptr.wrapping_add(ind + 1) }.swap_bytes() as u64;
                            let size =
                                ((unsafe { *ptr.wrapping_add(ind + 2) }.swap_bytes() as u64) << 32)
                                    | unsafe { *ptr.wrapping_add(ind + 3) }.swap_bytes() as u64;

                            write!(
                                def_uart,
                                "\nreserve base: 0x{:x}, size: 0x{:x}",
                                base_addr, size
                            )
                            .unwrap();
                        }
                    });
            }
            _ => {
                def_uart.puts("\nUnknown command: '");
                def_uart.puts(cmd);
                def_uart.putc(b'\'');
            }
        }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    use crate::uart;
    let mut uart_dev = uart::Uart::new(platform::UART_BASE as usize, platform::UART_REG_SHIFT);
    uart_dev.puts("Something went wrong!\n");
    if let Some(message) = info.message().as_str() {
        uart_dev.puts(message);
    } else {
        uart_dev.puts("No error message\n");
    }

    if let Some(locatioin) = info.location() {
        uart_dev.puts(locatioin.file());
        uart_dev.puts(" ");
        uart_dev.put_hex(locatioin.line() as u64);
        uart_dev.puts("\n");
    } else {
        uart_dev.puts("No location message\n");
    }
    loop {}
}
