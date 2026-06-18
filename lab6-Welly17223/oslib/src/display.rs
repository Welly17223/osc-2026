use crate::{
    interrupt,
    once::Once,
    virtual_mem::{self, phy_to_virt},
};
use core::{arch::asm, cell::OnceCell, ptr};

// const FB_BASE: usize = 0x87000000;
const FB_PHY_BASE: usize = 0x7f700000;
const FB_WIDTH: usize = 1920;
const FB_HEIGHT: usize = 1080;
const FB_BPP: usize = 4;
const XRGB8888: usize = 875713112;
pub static FB_BASE: Once<usize> = crate::once::Once::new();

struct RAMFBCfg {
    addr: u64,
    fourcc: u32,
    flags: u32,
    width: u32,
    height: u32,
    stride: u32,
}

const FW_CFG_BASE: usize = 0x10100000;
const FW_CFG_SELECT: *mut u16 = (FW_CFG_BASE + 0x08) as *mut u16;
const FW_CFG_DATA: *mut u64 = (FW_CFG_BASE + 0x00) as *mut u64;
const FW_CFG_DMA: *mut u64 = (FW_CFG_BASE + 0x10) as *mut u64;

const FW_CFG_DMA_CTL_ERROR: usize = 0x01;
const FW_CFG_DMA_CTL_READ: usize = 0x02;
const FW_CFG_DMA_CTL_SKIP: usize = 0x04;
const FW_CFG_DMA_CTL_SELECT: usize = 0x08;
const FW_CFG_DMA_CTL_WRITE: usize = 0x10;

const FW_CFG_FILE_DIR: usize = 0x19;
const CACHE_BLOCK_SIZE: usize = 64;

fn flush_dcache(addr: *mut u32, len: usize) {
    let start: usize = (addr as usize) & !(CACHE_BLOCK_SIZE - 1);
    for line in (start..(addr.wrapping_add(len)) as _).step_by(CACHE_BLOCK_SIZE) {
        unsafe { asm!("cbo.flush 0({})", in(reg) line) };
    }
}

pub fn video_bmp_display(bmp_image: *mut u32, width: usize, height: usize) {
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
}
