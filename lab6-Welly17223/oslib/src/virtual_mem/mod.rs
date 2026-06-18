use core::alloc::Layout;
use core::arch::asm;
use core::fmt::Write;
use core::ops::Index;
use core::ops::IndexMut;
use core::panic;

use crate::align;
use crate::memory_alloc;
use crate::uart::Uart;

/* VA bit-field shifts (Sv39) */
pub const PGD_SHIFT: u32 = 30;
pub const PMD_SHIFT: u32 = 21;
pub const PTE_SHIFT: u32 = 12;

/* Memory map */
pub const PAGE_OFFSET: usize = 0xffff_ffc0_0000_0000;
pub const PAGE_VPN2_OFFSET: usize = vpn2(PAGE_OFFSET);
pub const PGD_SIZE: usize = 1 << PGD_SHIFT;
pub const PMD_SIZE: usize = 1 << PMD_SHIFT;
pub const PAGE_SIZE: usize = 1 << PTE_SHIFT;

pub const PGD_MASK: usize = !(PGD_SIZE - 1);
pub const PMD_MASK: usize = !(PMD_SIZE - 1);
pub const PAGE_MASK: usize = !(PAGE_SIZE - 1);

pub const ENTRIES_PER_TABLE: usize = 512;

pub const KERNEL_PGD_INDEX: usize = (PAGE_OFFSET >> PGD_SHIFT) & 0x1FF;

pub const LINEAR_MAP_GIB: usize = 4;
pub const LINEAR_MAP_MIB: usize = 8;

/* PTE descriptor bits (Sv39) */
pub const PROP_MASK: usize = (1 << 10) - 1;
pub const PTE_V: usize = 1 << 0;
pub const PTE_R: usize = 1 << 1;
pub const PTE_W: usize = 1 << 2;
pub const PTE_X: usize = 1 << 3;
pub const PTE_U: usize = 1 << 4;
pub const PTE_G: usize = 1 << 5;
pub const PTE_A: usize = 1 << 6;
pub const PTE_D: usize = 1 << 7;

// Fork read only bits
pub const PTE_F: usize = 1 << 8;
// mmap marked
pub const PTE_M: usize = 1 << 9;

pub const SATP_SV39: usize = 8 << 60;
pub const PROT_KERNEL: usize = PTE_V | PTE_R | PTE_W | PTE_X | PTE_G | PTE_A | PTE_D;
pub const PROT_MMIO: usize = PTE_V | PTE_R | PTE_W | PTE_G | PTE_A | PTE_D;
pub const PROT_RD_ONLY: usize = PTE_V | PTE_R | PTE_G | PTE_A | PTE_D;

pub const USER_MODE_START_ADDRESS: usize = 0;
pub const USER_MODE_STACK_ADDRESS: usize = 0x0040_0000_0000;
pub const PROT_USER_TEXT: usize = PTE_V | PTE_R | PTE_X | PTE_U | PTE_A | PTE_D;
pub const PROT_USER_STACK: usize = PTE_V | PTE_R | PTE_W | PTE_U | PTE_A | PTE_D;

pub static mut VIRT_MAP_BEGIN: usize = 0;
pub static mut VIRT_IO_REMAN_BEGIN: usize = 0;

pub fn phy_begin() -> usize {
    unsafe { VIRT_MAP_BEGIN }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct PageTableEntry(pub usize);

impl PageTableEntry {
    #[inline]
    pub fn new(pa: usize, flags: usize) -> Self {
        Self(((pa & PAGE_MASK) >> 2) | flags)
    }

    pub fn new_leaf(pa: usize) -> Self {
        Self::new(pa, PTE_V)
    }

    #[inline]
    pub fn set_prop(&mut self, prop: usize) {
        let tmp = self.0 & !PROP_MASK;
        self.0 = tmp | (prop & PROP_MASK);
    }

    #[inline]
    pub fn set_pa(&mut self, pa: usize) {
        self.0 |= (pa & PAGE_MASK) >> 2;
    }

    #[inline]
    pub fn get_prop(&self) -> usize {
        self.0 & PROP_MASK
    }

    #[inline]
    pub fn get_pa(&self) -> usize {
        (self.0 & !PROP_MASK) << 2
    }

    #[inline]
    pub fn clear(&mut self) {
        self.0 = 0;
    }

    #[inline]
    pub fn is_set(&self, flags: usize) -> bool {
        self.0 & flags != 0
    }

    #[inline]
    pub fn is_leaf(&self) -> bool {
        self.0 & PROP_MASK == PTE_V
    }

    #[inline]
    pub fn is_valid(&self) -> bool {
        self.is_set(PTE_V)
    }

    #[inline]
    pub fn to_leaf_ref(&mut self) -> Option<&mut PageTable> {
        if self.is_leaf() {
            Some(unsafe { &mut *(phy_to_virt(self.get_pa()) as *mut PageTable) })
        } else {
            None
        }
    }
}

impl Drop for PageTable {
    fn drop(&mut self) {
        extern crate alloc;
        use alloc::boxed::Box;
        if self[256] == unsafe { PGD[256] } {
            self.entries[..256].iter_mut()
        } else {
            self.entries.iter_mut()
        }
        .for_each(|elem| {
            if let Some(leaf) = elem.to_leaf_ref() {
                drop(unsafe { Box::from_raw(leaf as _) })
            } else if elem.is_valid() {
                unsafe {
                    alloc::alloc::dealloc(
                        phy_to_virt(elem.get_pa()) as _,
                        Layout::new::<PageTableEntry>(),
                    );
                }
            }
        });
    }
}

#[repr(C, align(4096))]
pub struct PageTable {
    pub entries: [PageTableEntry; ENTRIES_PER_TABLE],
}

impl Clone for PageTable {
    fn clone(&self) -> Self {
        extern crate alloc;
        use alloc::boxed::Box;
        let mut pt = Self {
            entries: self.entries,
        };

        if pt[256] == unsafe { PGD[256] } {
            pt.entries[0..256].iter_mut()
        } else {
            pt.entries.iter_mut()
        }
        .for_each(|elem| {
            if let Some(leaf) = elem.to_leaf_ref() {
                let new_elem = Box::from(leaf.clone());
                *elem = PageTableEntry::new(virt_to_phy(Box::into_raw(new_elem) as _), PTE_V);
            }
        });

        pt.add_ref_count();
        pt
    }
}

impl Index<usize> for PageTable {
    type Output = PageTableEntry;

    fn index(&self, index: usize) -> &Self::Output {
        &self.entries[index]
    }
}

impl IndexMut<usize> for PageTable {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.entries[index]
    }
}

impl Default for PageTable {
    fn default() -> Self {
        Self {
            entries: [PageTableEntry(0); ENTRIES_PER_TABLE],
        }
    }
}

impl PageTable {
    pub fn iter(&self) -> core::slice::Iter<'_, PageTableEntry> {
        self.entries.iter()
    }

    pub fn iter_mut(&mut self) -> core::slice::IterMut<'_, PageTableEntry> {
        self.entries.iter_mut()
    }

    pub fn try_new_entry(&mut self, idx: usize, shift: u32) -> &mut Self {
        extern crate alloc;
        use alloc::boxed::Box;
        let entry = &mut self[idx];

        if !entry.is_leaf() {
            let phy_base = entry.get_pa();
            let curr_prot = entry.get_prop();

            let mut new_leaf = Box::new(PageTable::default());
            new_leaf.iter_mut().enumerate().for_each(|(idx, elem)| {
                *elem = PageTableEntry::new(phy_base + (idx << shift), curr_prot);
            });

            let new_pte = Box::into_raw(new_leaf);
            *entry = PageTableEntry::new_leaf(virt_to_phy(new_pte as _));
            unsafe {
                asm!("sfence.vma");
            }
        }

        unsafe { &mut *(phy_to_virt(entry.get_pa()) as *mut Self) }
    }

    pub fn set_prop_range(&mut self, start: usize, end_eq: usize, prop: usize) {
        assert!(start <= end_eq);
        self.entries[start..=end_eq].iter_mut().for_each(|elem| {
            if let Some(leaf) = elem.to_leaf_ref() {
                leaf.set_prop_range(0, ENTRIES_PER_TABLE - 1, prop);
            } else if elem.is_valid() {
                elem.set_prop(prop);
            }
        });
    }

    pub fn add_ref_count(&mut self) {
        self.entries.iter_mut().for_each(|elem| {
            if elem.is_leaf() {
                unsafe { &mut *(phy_to_virt(elem.get_pa()) as *mut PageTable) }.add_ref_count();
            } else if elem.is_valid() {
                memory_alloc::ALLOCATOR.increase_ref_count(elem.get_pa());
            }
        });
    }

    pub fn set_fork_prop(&mut self, start: usize, end_eq: usize) {
        assert!(start <= end_eq);
        self.entries[start..=end_eq].iter_mut().for_each(|elem| {
            if let Some(leaf) = elem.to_leaf_ref() {
                leaf.set_fork_prop(0, leaf.entries.len() - 1);
            } else if elem.is_valid() && elem.is_set(PTE_W) {
                elem.set_prop((elem.get_prop() & (!PTE_W)) | PTE_F);
            }
        });
    }
}

pub static mut PGD: PageTable = PageTable {
    entries: [PageTableEntry(0); 512],
};

#[inline]
pub const fn vpn2(addr: usize) -> usize {
    (addr >> PGD_SHIFT) & 0x1ff
}

#[inline]
pub const fn vpn1(addr: usize) -> usize {
    (addr >> PMD_SHIFT) & 0x1ff
}

#[inline]
pub const fn vpn0(addr: usize) -> usize {
    (addr >> PTE_SHIFT) & 0x1ff
}

#[inline]
pub const fn vpn(addr: usize, shift: u32) -> usize {
    (addr >> shift) & 0x1ff
}

#[inline]
pub fn make_satp(pa: usize) -> usize {
    (pa >> 12) | SATP_SV39
}

#[inline]
pub fn virt_to_phy(va: usize) -> usize {
    va - PAGE_OFFSET + phy_begin()
}

#[inline]
pub fn phy_to_virt(pa: usize) -> usize {
    pa - phy_begin() + PAGE_OFFSET
}

#[inline]
pub fn root_pgd_clone() -> PageTable {
    let ptr = &raw const PGD;
    let mut pgd_clone = PageTable {
        entries: [PageTableEntry(0); 512],
    };
    pgd_clone.entries[256..].copy_from_slice(&unsafe { &*ptr }.entries[256..]);

    pgd_clone.set_fork_prop(0, 255);
    pgd_clone
}

#[inline]
pub fn virt_shift_align(shift: u32) -> u32 {
    match shift {
        t if t <= PTE_SHIFT => PTE_SHIFT,
        t if t <= PMD_SHIFT && t > PTE_SHIFT => PMD_SHIFT,
        _ => PGD_SHIFT,
    }
}

pub fn load_user_program(root_pgd: &mut PageTable, user_program: &[u8]) {
    extern crate alloc;
    use alloc::boxed::Box;
    use alloc::vec::Vec;
    let mut curr_size = 0;
    let mut curr_shift;

    // move user program to pgd
    while curr_size < user_program.len() {
        let left_size = user_program.len() - curr_size;
        let curr_addr = USER_MODE_START_ADDRESS + curr_size;

        let pte = match left_size {
            ..PMD_SIZE => {
                curr_shift = PTE_SHIFT;
                let pmd = root_pgd.try_new_entry(vpn2(curr_addr), PMD_SHIFT);
                let pte = pmd.try_new_entry(vpn1(curr_addr), PTE_SHIFT);
                &mut pte[vpn0(curr_addr)]
            }
            PMD_SIZE..PGD_SIZE => {
                curr_shift = PMD_SHIFT;
                let pmd = root_pgd.try_new_entry(vpn2(curr_addr), PMD_SHIFT);
                &mut pmd[vpn1(curr_addr)]
            }
            PGD_SIZE.. => {
                curr_shift = PGD_SHIFT;
                &mut root_pgd[vpn2(curr_addr)]
            }
        };

        let end_size = curr_size + (1 << curr_shift);
        let page: Box<[u8]> = if end_size > user_program.len() {
            let mut new_box = Vec::with_capacity(end_size - curr_size);
            user_program[curr_size..user_program.len()]
                .iter()
                .for_each(|i| new_box.push(*i));
            (user_program.len()..end_size).for_each(|_| new_box.push(0));
            Box::from(new_box)
        } else {
            Box::from(&user_program[curr_size..end_size])
        };
        let page_ptr = Box::into_raw(page) as *const () as _;
        *pte = PageTableEntry::new(virt_to_phy(page_ptr), PROT_USER_TEXT);
        curr_size = end_size;
    }
}

#[unsafe(no_mangle)]
extern "C" fn init_virtual_memory(dtb_addr: u64, kernel_start: usize, kernel_end: usize) {
    // identity mapping
    // kernel_startup mapping
    let offset = PAGE_OFFSET + 0x200000 - kernel_start;
    for i in (kernel_start & PGD_MASK..align(kernel_end, PGD_SIZE)).step_by(1 << PGD_SHIFT) {
        unsafe {
            PGD[vpn2(i)] = PageTableEntry::new(i, PROT_KERNEL);
            PGD[vpn2(offset + i)] = PageTableEntry::new(i, PROT_KERNEL);
        }
    }

    // fdt mapping
    unsafe {
        PGD[vpn2(offset + dtb_addr as usize)] =
            PageTableEntry::new((dtb_addr as usize) & PGD_MASK, PROT_KERNEL);
    }

    let satp = make_satp(&raw const PGD as _);
    unsafe {
        asm!(
            r#"
            csrw satp, {}
            sfence.vma
            "#,
            in(reg) satp
        );
    }
}

#[unsafe(no_mangle)]
extern "C" fn drop_identity(kernel_start: usize, kernel_end: usize) {
    // identity mapping
    for i in (kernel_start & !((1 << PGD_SHIFT) - 1)..align(kernel_end, 1 << PGD_SHIFT))
        .step_by(1 << PGD_SHIFT)
    {
        unsafe {
            PGD[vpn2(i)].clear();
        }
    }
}

// This function is call after the memory allocator is initilized
pub fn init_finder_granularity() {
    use crate::fdt;
    let dtb_addr = unsafe { fdt::DTB_ADDR } as _;
    let mut reserved_memory_node = [0usize; 64];
    let reserved_memory_node_num =
        fdt::path_all_offset(dtb_addr, "/reserved-memory/*", &mut reserved_memory_node).unwrap();

    for idx in &reserved_memory_node[..reserved_memory_node_num] {
        let (ptr, len) = match fdt::getprop(dtb_addr, *idx, "reg") {
            Ok(v) => v,
            Err(fdt::Error::Notfound) => continue,
            Err(e) => panic!("Unexpected error {e:#?}"),
        };

        let len = len / size_of::<u64>() / 2;
        let ptr = ptr as *const u32;

        for i in 0..len {
            let mem_off = i << 2;

            let phy_base =
                unsafe { crate::read_u64_from_ptr_swapbyte(ptr.wrapping_add(mem_off)) } as usize;
            let size = unsafe { crate::read_u64_from_ptr_swapbyte(ptr.wrapping_add(mem_off + 2)) }
                as usize;

            set_memory_prop(phy_base, size, PROT_RD_ONLY);
        }
    }
}

pub fn set_memory_prop(phy_base: usize, size: usize, prop: usize) {
    extern crate alloc;
    let virt_base = phy_base - phy_begin() + PAGE_OFFSET;
    // last page counted
    let virt_end = align(virt_base + size, 0x1000) - 0x1000;

    let pgd_ptr = &raw mut PGD;

    match size {
        0..PMD_SIZE => {
            let pmd = unsafe { (&mut *pgd_ptr).try_new_entry(vpn2(virt_base), PMD_SHIFT) };
            let pte = pmd.try_new_entry(vpn1(virt_base), PTE_SHIFT);
            pte.set_prop_range(vpn0(virt_base), vpn0(virt_end), prop);
        }
        PMD_SIZE.. => {
            for gb_idx in vpn2(virt_base)..=vpn2(virt_end) {
                let mut mb_start = if gb_idx == vpn2(virt_base) {
                    vpn1(virt_base)
                } else {
                    0
                };
                let mut mb_end = if gb_idx == vpn2(virt_end) {
                    vpn1(virt_end)
                } else {
                    ENTRIES_PER_TABLE - 1
                };

                if mb_start == 0 && mb_end == ENTRIES_PER_TABLE - 1 {
                    unsafe {
                        PGD[gb_idx].set_prop(prop);
                    };
                    continue;
                }

                let pmd = unsafe { (&mut *pgd_ptr).try_new_entry(gb_idx, PMD_SHIFT) };
                if gb_idx == vpn2(virt_base) && virt_base & (!PMD_MASK) != 0 {
                    let pte = pmd.try_new_entry(vpn1(virt_base), PTE_SHIFT);
                    pte.set_prop_range(vpn0(virt_base), ENTRIES_PER_TABLE - 1, prop);
                    mb_start += 1;
                }

                if gb_idx == vpn2(virt_end) && virt_end & (!PMD_MASK) != 0 {
                    let pte = pmd.try_new_entry(vpn1(virt_end), PTE_SHIFT);
                    pte.set_prop_range(0, vpn0(virt_end), prop);
                    mb_end -= 1;
                }

                if mb_start <= mb_end {
                    pmd.set_prop_range(mb_start, mb_end, prop);
                }
            }
        } // PGD_SIZE.. => for gb_idx in vpn2(virt_base)..=vpn2(virt_end) {},
    }
}

pub fn io_remap(phy_base: usize, size: usize) -> usize {
    let io_remap_curr = unsafe { VIRT_IO_REMAN_BEGIN };
    let aligned_phy_base = phy_base & PMD_MASK;

    let size = align(size, PMD_SIZE);
    let pgd_ptr = &raw mut PGD;

    for offset in (0..size).step_by(PMD_SIZE) {
        let pmd_entry =
            unsafe { (*pgd_ptr).try_new_entry(vpn2(io_remap_curr + offset), PGD_SHIFT) };
        pmd_entry[vpn1(io_remap_curr + offset)] =
            PageTableEntry::new(aligned_phy_base + offset, PROT_MMIO);
    }

    unsafe { VIRT_IO_REMAN_BEGIN += size };
    io_remap_curr + (phy_base - aligned_phy_base)
}

pub fn pagewalk(root_pgd: *mut PageTable, va: usize, pa: usize, prop: usize) {
    let va = va & PAGE_MASK;
    let pa = pa & PAGE_MASK;
    let mut pte_ptr = root_pgd;
    let mut curr_shift = PGD_SHIFT;
    let vpn = |va: usize, shift: u32| (va >> shift) & 0x1ff;

    for _i in 0..2 {
        pte_ptr = unsafe { &mut (*pte_ptr) }.try_new_entry(vpn(va, curr_shift), curr_shift - 9);
        curr_shift -= 9;
    }

    let entry = &mut unsafe { &mut *pte_ptr }.entries[vpn(va, curr_shift)];
    if entry.is_valid() {
        entry.set_prop(prop);
    } else {
        *entry = PageTableEntry::new(pa, prop);
    }
}
