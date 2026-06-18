use core::{ffi::c_char, ptr};

use crate::uart;

const MAGIC: &[u8; 6] = b"070701";

#[repr(C)]
pub struct Cpio {
    pub magic: [c_char; 6],
    pub ino: [c_char; 8],
    pub mode: [c_char; 8],
    pub uid: [c_char; 8],
    pub gid: [c_char; 8],
    pub nlink: [c_char; 8],
    pub mtime: [c_char; 8],
    pub filesize: [c_char; 8],
    pub devmajor: [c_char; 8],
    pub devminor: [c_char; 8],
    pub rdevmajor: [c_char; 8],
    pub rdevminor: [c_char; 8],
    pub namesize: [c_char; 8],
    pub check: [c_char; 8],
}

struct CpioIter {
    cpio: *mut Cpio,
}

impl Iterator for CpioIter {
    type Item = (&'static str, &'static [u8]);
    fn next(&mut self) -> Option<Self::Item> {
        if unsafe { (*self.cpio).magic } != *MAGIC {
            return None;
        }

        let name_size = hextoi(&unsafe { (*self.cpio).namesize }) as usize;
        let file_size = hextoi(&unsafe { (*self.cpio).filesize }) as usize;

        let name = unsafe {
            ptr::slice_from_raw_parts(
                self.cpio.wrapping_byte_offset(size_of::<Cpio>() as isize) as *const u8,
                name_size - 1,
            )
            .as_ref()
        }?;

        if name.len() >= 10 && &name[..10] == b"TRAILER!!!" && file_size == 0 {
            return None;
        }

        self.cpio = self
            .cpio
            .wrapping_byte_offset((size_of::<Cpio>() + name_size) as isize);
        self.cpio = align(self.cpio as u64, 4) as *mut Cpio;

        let name = unsafe { core::str::from_utf8_unchecked(name) };
        let file = unsafe { core::slice::from_raw_parts(self.cpio as *const u8, file_size) };

        self.cpio = self.cpio.wrapping_byte_offset((file_size) as isize);
        self.cpio = align(self.cpio as u64, 4) as *mut Cpio;

        Some((name, file))
    }
}

#[derive(Debug)]
pub enum CatError {
    FileNotFound,
    DecodeError,
    CPIPOMagicMismatching,
}

#[inline]
pub fn hextoi(s: &[u8]) -> u32 {
    let mut r = 0u32;
    for c in s {
        r <<= 4;
        if *c >= b'A' {
            r += (*c - b'A' + 10) as u32;
        } else {
            r += (*c - b'0') as u32;
        }
    }
    r
}

#[inline]
fn align(n: u64, byte: u32) -> u64 {
    (n + byte as u64 - 1) & (!(byte as u64 - 1))
}

pub fn list(uart_dev: &uart::Uart, cpio_addr: *const Cpio) -> Result<(), CatError> {
    let iter = CpioIter {
        cpio: cpio_addr as *mut Cpio,
    };
    for (file_name, file) in iter {
        uart_dev.put_dec(file.len() as u64);
        uart_dev.puts(" ");
        uart_dev.puts(file_name);
        uart_dev.puts("\n");
    }
    Ok(())
}

pub fn cat<T>(uart_dev: &uart::Uart, cpio_addr: *const Cpio, filename: T) -> Result<(), CatError>
where
    T: AsRef<[u8]>,
{
    let iter = CpioIter {
        cpio: cpio_addr as *mut Cpio,
    };
    for (file_name, file) in iter {
        if file_name.as_bytes() == filename.as_ref() {
            uart_dev.puts(file_name);
            uart_dev.puts(":\n");
            uart_dev.puts(file);
            return Ok(());
        }
    }
    Err(CatError::FileNotFound)
}
