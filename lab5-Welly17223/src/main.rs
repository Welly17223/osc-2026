#![no_std]
#![no_main]
extern crate alloc;

use core::arch::asm;
use core::{arch::global_asm, panic::PanicInfo};
use core::str;

use log::{info, trace, warn};
use memory_alloc::ALLOCATOR;
use oslib::interrupt::{self, timer};
use oslib::ramdisk::{Cpio, INITRD_START};
use oslib::thread::idle_thread;
use oslib::uart::{self, SERIAL, Uart};
use oslib::{fdt, logger, platform, schedule};

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

#[allow(static_mut_refs)]
#[unsafe(no_mangle)]
pub extern "C" fn main(hart_id: u64, dtb_addr: u64) {
    unsafe { fdt::DTB_ADDR = dtb_addr as _ };
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
    schedule::init();

    // timer::add_timer::<u8>(timer::Time::new(5, TimeUnit::Sec), boot_func, None, true);
    interrupt::s_mode_interrupt_enable();

    idle_thread();
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
