extern crate alloc;
use alloc::{boxed::Box, vec, vec::Vec};

use crate::{
    schedule, thread, uart,
    virtual_mem::{
        self, PAGE_MASK, PTE_F, PTE_M, PTE_V, PTE_W, pagewalk, phy_to_virt, virt_to_phy,
    },
};

use core::{arch::asm, cmp::min, fmt::Write, ptr};

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

        let curr_pgd = if let Some(pgd) = tcb.pgd.as_mut() {
            &mut pgd.as_mut().entries
        } else {
            let ptr = &raw mut virtual_mem::PGD;
            &mut unsafe { &mut *ptr }.entries
        };

        let mut curr_shift = virtual_mem::PGD_SHIFT;
        let va = regs.stval;
        let mut leaf_pte = &raw mut curr_pgd[virtual_mem::vpn(va, curr_shift)];

        while let Some(leaf) = unsafe { &mut *leaf_pte }.to_leaf_ref() {
            curr_shift -= 9;
            leaf_pte = &raw mut leaf[virtual_mem::vpn(va, curr_shift)];
        }

        let leaf_pte = unsafe { &mut *leaf_pte };
        let leaf_prop = leaf_pte.get_prop();

        if va < tcb.text.target.len() && !leaf_pte.is_valid() {
            let buttom = va & PAGE_MASK;
            let size = min(4096, tcb.text.target.len() - buttom);
            let mut part_text = Box::new([0u8; 4096]);
            part_text[..size].copy_from_slice(&tcb.text.target[buttom..buttom + size]);

            let root_pgd = tcb.pgd.as_mut().unwrap().as_mut().get_mut();

            pagewalk(
                root_pgd,
                buttom,
                virt_to_phy(Box::into_raw(part_text) as _),
                virtual_mem::PROT_USER_TEXT,
            );

            unsafe {
                asm!("sfence.vma");
            }
            return;
        }

        if leaf_pte.is_set(PTE_M) {
            leaf_pte.set_prop(leaf_prop & (!PTE_M) | PTE_V);
            let page = vec![0u8; 1 << curr_shift];
            let (ptr, _, _) = Vec::into_raw_parts(page);
            leaf_pte.set_pa(virtual_mem::virt_to_phy(ptr as _));
            unsafe {
                asm!("sfence.vma");
            }
            return;
        }

        if !leaf_pte.is_valid() {
            writeln!(serial, "[Segmentation fault]: Kill Process").unwrap();
            thread::do_exit(1);
        }

        match interrupt_state.unwrap() {
            PageFault::StoreAMO if leaf_pte.is_set(PTE_F) => {
                let new_prop = (leaf_pte.get_prop() & !PTE_F) | PTE_W;
                let old_page = unsafe {
                    &*ptr::slice_from_raw_parts(
                        phy_to_virt(leaf_pte.get_pa()) as *mut u8,
                        1 << curr_shift,
                    )
                };
                let page: Box<[u8]> = Box::from(old_page);
                *leaf_pte = virtual_mem::PageTableEntry::new(
                    virtual_mem::virt_to_phy(Box::into_raw(page) as *const () as _),
                    new_prop,
                );
                writeln!(serial, "[Permission fault]: {:#x}", va).unwrap();
                let va_aligned = va & virtual_mem::PAGE_MASK;
                unsafe {
                    asm!("sfence.vma {}, zero", in(reg) va_aligned);
                }
            }
            _ => {
                writeln!(serial, "[Segmentation fault]: Kill Process").unwrap();
                thread::do_exit(1);
            }
        }
    }
}
