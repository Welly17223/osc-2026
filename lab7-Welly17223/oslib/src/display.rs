use crate::{
    file_system::VfsError,
    interrupt,
    once::Once,
    virtual_mem::{self},
};
use core::{arch::asm, cmp::min, ffi::c_uint, ptr};

extern crate alloc;
use alloc::sync::Arc;

// const FB_BASE: usize = 0x87000000;
const FB_PHY_BASE: usize = 0x7f700000;
const FB_WIDTH: usize = 1920;
const FB_HEIGHT: usize = 1080;
const FB_BPP: usize = 4;
pub static FB_BASE: Once<usize> = crate::once::Once::new();

const CACHE_BLOCK_SIZE: usize = 64;

#[derive(Clone, Copy)]
pub struct DisplayBuffer {
    base: usize,
    width: usize,
    height: usize,
    bpp: usize,
    cache_block_size: usize,
}

#[repr(C)]
struct FramBufferInfo {
    width: c_uint,
    height: c_uint,
    bpp: c_uint,
}

pub static DISPLAY: Once<DisplayBuffer> = Once::new();

impl DisplayBuffer {
    fn flush_dcache(&self, addr: *mut u32, len: usize) {
        let start: usize = (addr as usize) & !(self.cache_block_size - 1);
        for line in (start..(addr.wrapping_byte_add(len)) as _).step_by(self.cache_block_size) {
            unsafe { asm!("cbo.flush 0({})", in(reg) line) };
        }
    }

    pub fn video_bmp_display(&self, bmp_image: *mut u32, width: usize, height: usize) {
        let fb = self.base as *mut u32;
        let start_x = (self.width - width) / 2;
        let start_y = (self.height - height) / 2;
        for y in 0..height {
            let dst = fb.wrapping_add((start_y + y) * self.width + start_x);
            unsafe {
                let _status_sum = interrupt::SetStatusSUM::new();
                ptr::copy_nonoverlapping(bmp_image.wrapping_add(y * width), dst, width);
            }
            self.flush_dcache(dst, width * size_of::<u32>());
        }
    }
}

impl crate::file_system::byte_device::ByteDevice for DisplayBuffer {
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize, crate::file_system::VfsError> {
        Err(crate::file_system::VfsError::IoError)
    }

    fn write(&self, offset: u64, buf: &[u8]) -> Result<usize, crate::file_system::VfsError> {
        unsafe {
            ptr::copy_nonoverlapping(
                buf.as_ptr() as _,
                (self.base as *mut u8).wrapping_offset(offset as isize),
                buf.len(),
            );
        }
        self.flush_dcache((self.base + offset as usize) as _, buf.len());
        Ok(buf.len())
    }

    fn ioctl(&self, requests: usize, ptr: *mut ()) -> Result<(), crate::file_system::VfsError> {
        match requests {
            0 => {
                unsafe {
                    (ptr as *mut FramBufferInfo).write_volatile(FramBufferInfo {
                        width: self.width as _,
                        height: self.height as _,
                        bpp: self.bpp as _,
                    });
                }
                Ok(())
            }
            _ => Err(VfsError::Unimplemented),
        }
    }

    fn seek(
        &self,
        _vnode: Arc<crate::file_system::Vnode>,
        f_pos: u64,
        pos: crate::file_system::SeekFrom,
    ) -> Result<u64, VfsError> {
        let end = self.width * self.height * self.bpp;
        let n = match pos {
            crate::file_system::SeekFrom::Start(o) => o,
            crate::file_system::SeekFrom::Current(o) => f_pos.saturating_add_signed(o),
            crate::file_system::SeekFrom::End(o) => (end as u64).saturating_add_signed(o),
        };
        let n = min(n, end as u64);
        Ok(n)
    }
}

pub fn init_display() {
    let virt_addr = virtual_mem::io_remap(FB_PHY_BASE, FB_WIDTH * FB_HEIGHT * FB_BPP);
    let _ = DISPLAY.set(DisplayBuffer {
        base: virt_addr,
        width: FB_WIDTH,
        height: FB_HEIGHT,
        bpp: FB_BPP,
        cache_block_size: CACHE_BLOCK_SIZE,
    });
}

/* pub fn video_bmp_display(bmp_image: *mut u32, width: usize, height: usize) {
    let fb = *FB_BASE
        .get_or_init(|| virtual_mem::io_remap(FB_PHY_BASE, FB_WIDTH * FB_HEIGHT * FB_BPP))
        as *mut u32;
    let start_x = (FB_WIDTH - width) / 2;
    let start_y = (FB_HEIGHT - height) / 2;
    for y in 0..height {
        let dst = fb.wrapping_add((start_y + y) * FB_WIDTH + start_x);
        unsafe {
            let _status_sum = interrupt::SetStatusSUM::new();
            ptr::copy_nonoverlapping(bmp_image.wrapping_add(y * width), dst, width);
        }
        flush_dcache(dst, width * size_of::<u32>());
    }
} */

pub fn video_bmp_display(bmp_image: *mut u32, width: usize, height: usize) {
    let disp = DISPLAY.get().unwrap();
    disp.video_bmp_display(bmp_image, width, height);
}
