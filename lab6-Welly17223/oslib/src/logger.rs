use core::fmt::Write;
use log::Log;

use crate::uart;

pub struct Logger {}

pub static LOGGER: Logger = Logger {};

impl Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        !metadata.target().contains("memory_alloc")
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        extern crate alloc;
        let serial_contaienr = &raw mut uart::SERIAL;

        if let Some(l) = unsafe { &mut *serial_contaienr } {
            writeln!(
                l,
                "[{}] {}: {}",
                record.target(),
                record.level(),
                record.args()
            )
            .unwrap();
        }
    }

    fn flush(&self) {}
}
