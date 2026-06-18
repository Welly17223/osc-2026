use core::fmt::Write;
use log::Log;
use spin::mutex;
use riscv::register::sstatus;

use crate::uart;

pub struct Logger {}

pub static LOGGER: Logger = Logger {};
pub static UART_LOGGER: mutex::Mutex<Option<uart::Uart>> = mutex::Mutex::new(None);

impl Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        let is_enable = sstatus::read().sie();
        unsafe {
            sstatus::clear_sie();
        };
        if let Some(l) = UART_LOGGER.lock().as_mut() {
            writeln!(
                l,
                "[{}] {}: {}",
                record.target(),
                record.level(),
                record.args()
            )
            .unwrap();
        }

        if is_enable {
            unsafe {
                sstatus::set_sie();
            }
        }
    }

    fn flush(&self) {}
}
