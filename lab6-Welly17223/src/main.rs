#![no_std]
#![no_main]
extern crate alloc;

use core::{
    arch::{asm, global_asm},
    panic::PanicInfo,
    str,
};

use log::{info, trace, warn};

use oslib::{
    fdt,
    interrupt::{self, timer},
    logger,
    memory_alloc::ALLOCATOR,
    ramdisk::{Cpio, INITRD_START},
    schedule,
    thread::idle_thread,
    uart::{self, SERIAL},
    virtual_mem,
};

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
    let kernel_start: usize;
    let kernel_end: usize;
    let stack_buttom: usize;

    unsafe {
        asm!("la {}, __kernel_start; la {}, __kernel_end", out(reg) kernel_start, out(reg)kernel_end);
        asm!("la {}, __stack_buttom", out(reg) stack_buttom);
    }
    let memory_alloc = match oslib::memory_alloc::init_memory_allocator(
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
    virtual_mem::init_finder_granularity();

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
        INITRD_START = virtual_mem::phy_to_virt((linux_initrd_start as usize).into()).addr();
    }
    schedule::init();

    // timer::add_timer::<u8>(timer::Time::new(5, TimeUnit::Sec), boot_func, None, true);
    interrupt::s_mode_interrupt_enable();

    idle_thread();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    use core::fmt::Write;
    let def_uart = oslib::uart::get_serial();
    def_uart.disable_interrupt();
    def_uart.puts("Something went wrong!\n");

    writeln!(def_uart, "{}", info).unwrap();

    loop {
        unsafe { asm!("wfi") };
    }
}
