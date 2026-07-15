extern crate alloc;
use alloc::boxed::Box;

use crate::{
    schedule, thread, uart,
    virtual_mem::{self, PTE_W, VirtualAddress},
};

use core::{arch::asm, fmt::Write, ptr};

#[derive(PartialEq, Eq, Debug)]
pub enum PageFault {
    Instruction = 12,
    Load = 13,
    StoreAMO = 15,
}

impl TryFrom<usize> for PageFault {
    type Error = ();
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        match value {
            12 => Ok(Self::Instruction),
            13 => Ok(Self::Load),
            15 => Ok(Self::StoreAMO),
            _ => Err(()),
        }
    }
}

impl From<PageFault> for usize {
    fn from(val: PageFault) -> Self {
        val as _
    }
}

impl TryFrom<super::SupervisorInterrupt0> for PageFault {
    type Error = ();
    fn try_from(value: super::SupervisorInterrupt0) -> Result<Self, Self::Error> {
        use super::SupervisorInterrupt0;
        match value {
            SupervisorInterrupt0::InstructionPageFault => Ok(Self::Instruction),
            SupervisorInterrupt0::LoadPageFault => Ok(Self::Load),
            SupervisorInterrupt0::StoreAMOPageFault => Ok(Self::StoreAMO),
            _ => Err(()),
        }
    }
}

pub struct Interrupt {}

impl super::InterruptTrait for Interrupt {
    type InterruptEnum = PageFault;
    fn handler(regs: &mut super::pt_regs, interrupt_state: Option<Self::InterruptEnum>) {
        if interrupt_state.is_none() {
            return;
        }
        let serial = uart::get_serial();
        let tcb = schedule::current_tcb();
        writeln!(
            serial,
            "PageFault: sepc: {:#x}, stval: {:#x}, cause: {:?}",
            regs.sepc, regs.stval, interrupt_state
        )
        .unwrap();

        let va = VirtualAddress(regs.stval);
        match tcb.vm_mapper.as_mut().unwrap().map_to_phy(va) {
            Ok(()) => {
                writeln!(serial, "[Translation fault]: {:#x}", va.addr()).unwrap();
                let va_aligned = va & virtual_mem::PAGE_MASK;
                unsafe {
                    asm!("sfence.vma {}, zero", in(reg) va_aligned.addr());
                }
            }
            Err(virtual_mem::vm_area::Error::AlreadyMapPTE { prop_xor })
                if prop_xor & PTE_W != 0 && interrupt_state.unwrap() == PageFault::StoreAMO =>
            {
                writeln!(serial, "prop: {:#b}", prop_xor).unwrap();

                let leaf_pte = tcb.vm_mapper.as_mut().unwrap().page_entry_mut(va).unwrap();
                let new_prop = leaf_pte.get_prop() | PTE_W;
                let old_page = unsafe {
                    Box::from_raw(ptr::slice_from_raw_parts_mut(
                        leaf_pte.get_pa().into_virt().addr() as *mut u8,
                        0x1000,
                    ))
                };
                let page: Box<[u8]> = old_page.clone();
                *leaf_pte = virtual_mem::PageTableEntry::new(
                    VirtualAddress(Box::into_raw(page) as *const () as _).into_phy(),
                    new_prop,
                );
                writeln!(serial, "[Permission fault]: {:#x}", va).unwrap();
                let va_aligned = va & virtual_mem::PAGE_MASK;
                unsafe {
                    asm!("sfence.vma {}, zero", in(reg) va_aligned.addr());
                }
            }
            Err(e) => {
                let error_code: isize = e.into();
                writeln!(serial, "[Segmentation fault]: Kill Process").unwrap();
                thread::do_exit(-error_code);
            }
        };
    }
}
