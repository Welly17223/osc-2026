#[cfg(feature = "orangePI")]
pub const UART_BASE: *mut u8 = 0xD4017000 as *mut u8;
#[cfg(feature = "qemu")]
pub const UART_BASE: *mut u8 = 0x10000000 as *mut u8;

pub const UART_RBR: *mut u8 = UART_BASE.wrapping_byte_add(0x0);
pub const UART_THR: *mut u8 = UART_BASE.wrapping_byte_add(0x0);

#[cfg(feature = "orangePI")]
pub const UART_LSR: *mut u8 = UART_BASE.wrapping_byte_add(0x14);
#[cfg(feature = "qemu")]
pub const UART_LSR: *mut u8 = UART_BASE.wrapping_byte_add(0x5);
