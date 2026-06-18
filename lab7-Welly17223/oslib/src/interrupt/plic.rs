use core::ptr;

use crate::{
    fdt::{self},
    virtual_mem,
};

const ENABLE: usize = 0x002080;
const THRESHOLD: usize = 0x201000;
const CLAIM: usize = 0x201004;

pub static mut HART0_PLIC: Option<PLIC> = None;

#[derive(PartialEq)]
pub enum IRQ {
    UART,
}

pub fn init_plic(dtb_addr: *mut u8, hart_id: usize) -> Result<(), crate::fdt::Error> {
    let n = match crate::fdt::path_offset(dtb_addr, "/soc/interrupt-controller", 1) {
        Err(fdt::Error::Notfound) => fdt::path_offset(dtb_addr, "/soc/plic", 1)?,
        Ok(n) => n,
        Err(_) => panic!("Plic interrupt controller not found"),
    };
    let (competible, len) = crate::fdt::getprop(dtb_addr, n, "compatible")?;

    let competible =
        unsafe { str::from_utf8(&*ptr::slice_from_raw_parts(competible, len)) }.unwrap();

    if !competible.contains("plic") {
        unimplemented!();
    }

    let (res, _) = crate::fdt::getprop(dtb_addr, n, "reg")?;
    let (plic_phy_base, plic_len) = fdt::read_reg(res as _);

    let plic_virt_base = virtual_mem::io_remap(plic_phy_base as _, plic_len as _);
    let plic = PLIC {
        base: plic_virt_base,
    };

    // setup uart interrupt
    plic.set_threshold(hart_id, 0);

    // setup timer interrupt
    unsafe { HART0_PLIC = Some(plic) }

    Ok(())
}

pub struct PLIC {
    base: usize,
}

pub enum BitOp {
    Set,
    Clear,
}

impl PLIC {
    pub fn claim(&self, hart_id: usize) -> u32 {
        unsafe { ((self.base + CLAIM + hart_id * 0x2000) as *const u32).read_volatile() }
    }

    pub fn complete<T: Into<u32>>(&self, hart_id: usize, irq: T) {
        unsafe {
            ((self.base + CLAIM + hart_id * 0x2000) as *mut u32).write_volatile(irq.into());
        };
    }

    pub fn set_priority<T: Into<u32>>(&self, irq: T, priority: u32) {
        unsafe {
            (self.base as *mut u32)
                .wrapping_add(irq.into() as usize)
                .write_volatile(priority);
        };
    }

    pub fn enable(&self, hart_id: usize, bit_idx: usize) {
        let m = (self.base as *mut u32)
            .wrapping_byte_add(ENABLE + hart_id * 0x100)
            .wrapping_add(bit_idx / 32);
        unsafe {
            let mut tmp = m.read_volatile();
            tmp |= 1 << (bit_idx & 0x1f);
            m.write_volatile(tmp);
        }
    }

    pub fn set_threshold(&self, hart_id: usize, thres: u32) {
        unsafe {
            ((self.base + THRESHOLD + hart_id * 0x2000) as *mut u32).write_volatile(thres);
        };
    }

    pub fn get_base(&self) -> usize {
        self.base
    }
}
