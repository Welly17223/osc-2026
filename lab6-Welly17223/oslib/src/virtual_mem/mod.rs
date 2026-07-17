use core::{
    arch::asm,
    fmt::LowerHex,
    ops::{Add, BitAnd, Index, IndexMut, Sub},
    panic,
    slice::SliceIndex,
};

use riscv::register::satp::{self, Satp};

use crate::align;
use crate::memory_alloc;

pub mod vm_area;

/* VA bit-field shifts (Sv39) */
pub const PGD_SHIFT: u32 = 30;
pub const PMD_SHIFT: u32 = 21;
pub const PTE_SHIFT: u32 = 12;

/* Memory map */
pub const PAGE_OFFSET: VirtualAddress = VirtualAddress(0xffff_ffc0_0000_0000);
pub const PAGE_VPN2_OFFSET: usize = vpn2(PAGE_OFFSET);
pub const PGD_SIZE: usize = 1 << PGD_SHIFT;
pub const PMD_SIZE: usize = 1 << PMD_SHIFT;
pub const PAGE_SIZE: usize = 1 << PTE_SHIFT;

pub const PGD_MASK: usize = !(PGD_SIZE - 1);
pub const PMD_MASK: usize = !(PMD_SIZE - 1);
pub const PAGE_MASK: usize = !(PAGE_SIZE - 1);

pub const ENTRIES_PER_TABLE: usize = 512;

pub const KERNEL_PGD_INDEX: usize = (PAGE_OFFSET.addr() >> PGD_SHIFT) & 0x1FF;

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

pub const USER_MODE_START_ADDRESS: VirtualAddress = VirtualAddress(0);
pub const USER_MODE_STACK_ADDRESS: VirtualAddress = VirtualAddress(0x0040_0000_0000);
pub const PROT_USER_TEXT: usize = PTE_V | PTE_R | PTE_X | PTE_U | PTE_A | PTE_D;
pub const PROT_USER_STACK: usize = PTE_V | PTE_R | PTE_W | PTE_U | PTE_A | PTE_D;

pub static mut VIRT_MAP_BEGIN: usize = 0;
pub static mut VIRT_IO_REMAN_BEGIN: usize = 0;

pub fn phy_begin() -> usize {
    unsafe { VIRT_MAP_BEGIN }
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct VirtualAddress(pub usize);

impl LowerHex for VirtualAddress {
    #[doc = r" Format unsigned integers in the radix."]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        <usize as LowerHex>::fmt(&self.0, f)
    }
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct PhysicalAddress(pub usize);

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct PageTableEntry(pub usize);

impl VirtualAddress {
    #[inline]
    pub const fn addr(&self) -> usize {
        self.0
    }

    #[inline]
    pub fn into_phy(&self) -> PhysicalAddress {
        virt_to_phy(*self)
    }

    #[inline]
    pub const fn vpn2(&self) -> usize {
        vpn2(*self)
    }

    #[inline]
    pub const fn vpn1(&self) -> usize {
        vpn1(*self)
    }

    #[inline]
    pub const fn vpn0(&self) -> usize {
        vpn0(*self)
    }
}

impl From<usize> for VirtualAddress {
    fn from(value: usize) -> Self {
        VirtualAddress(value)
    }
}

impl From<u64> for VirtualAddress {
    fn from(value: u64) -> Self {
        VirtualAddress(value as usize)
    }
}

impl Add<usize> for VirtualAddress {
    type Output = Self;
    fn add(self, rhs: usize) -> Self::Output {
        Self(self.addr() + rhs)
    }
}

impl Add for VirtualAddress {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.addr() + rhs.addr())
    }
}

impl Sub<usize> for VirtualAddress {
    type Output = Self;
    fn sub(self, rhs: usize) -> Self::Output {
        VirtualAddress(self.0 - rhs)
    }
}

impl Sub for VirtualAddress {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        VirtualAddress(self.0 - rhs.0)
    }
}

impl BitAnd<usize> for VirtualAddress {
    type Output = Self;
    fn bitand(self, rhs: usize) -> Self::Output {
        Self(self.addr() & rhs)
    }
}

impl PhysicalAddress {
    pub const fn addr(&self) -> usize {
        self.0
    }

    pub fn into_virt(&self) -> VirtualAddress {
        phy_to_virt(*self)
    }
}

impl From<usize> for PhysicalAddress {
    fn from(value: usize) -> Self {
        PhysicalAddress(value)
    }
}

impl From<u64> for PhysicalAddress {
    fn from(value: u64) -> Self {
        PhysicalAddress(value as _)
    }
}

impl Add<usize> for PhysicalAddress {
    type Output = Self;
    fn add(self, rhs: usize) -> Self::Output {
        Self(self.addr() + rhs)
    }
}

impl Add for PhysicalAddress {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.addr() + rhs.addr())
    }
}

impl Sub<usize> for PhysicalAddress {
    type Output = Self;
    fn sub(self, rhs: usize) -> Self::Output {
        PhysicalAddress(self.0 - rhs)
    }
}
impl Sub for PhysicalAddress {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        PhysicalAddress(self.0 - rhs.0)
    }
}

impl BitAnd<usize> for PhysicalAddress {
    type Output = Self;
    fn bitand(self, rhs: usize) -> Self::Output {
        Self(self.addr() & rhs)
    }
}

impl PageTableEntry {
    #[inline]
    pub fn new(pa: PhysicalAddress, flags: usize) -> Self {
        Self(((pa.0 & PAGE_MASK) >> 2) | flags)
    }

    pub fn new_leaf(pa: PhysicalAddress) -> Self {
        Self::new(pa, PTE_V)
    }

    #[inline]
    pub fn set_prop(&mut self, prop: usize) {
        let tmp = self.0 & !PROP_MASK;
        self.0 = tmp | (prop & PROP_MASK);
    }

    #[inline]
    pub fn set_pa(&mut self, pa: PhysicalAddress) {
        self.0 |= (pa.0 & PAGE_MASK) >> 2;
    }

    #[inline]
    pub fn get_prop(&self) -> usize {
        self.0 & PROP_MASK
    }

    #[inline]
    pub fn get_pa(&self) -> PhysicalAddress {
        PhysicalAddress((self.0 & !PROP_MASK) << 2)
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
    pub fn to_leaf_mut(&mut self) -> Option<&mut PageTable> {
        if self.is_leaf() {
            Some(unsafe { &mut *(self.get_pa().into_virt().addr() as *mut PageTable) })
        } else {
            None
        }
    }

    #[inline]
    pub fn to_leaf_ref(&self) -> Option<&PageTable> {
        if self.is_leaf() {
            Some(unsafe { &*(self.get_pa().into_virt().addr() as *const PageTable) })
        } else {
            None
        }
    }
}

#[derive(Debug)]
#[repr(C, align(4096))]
pub struct PageTable {
    pub entries: [PageTableEntry; ENTRIES_PER_TABLE],
}

/* impl Clone for PageTable {
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
            if let Some(leaf) = elem.to_leaf_mut() {
                let new_elem = Box::from(leaf.clone());
                *elem = PageTableEntry::new(
                    VirtualAddress(Box::into_raw(new_elem) as usize).into_phy(),
                    PTE_V,
                );
            }
        });

        pt.add_ref_count();
        pt
    }
} */

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
            assert!(!entry.is_valid() || entry.is_set(PTE_G));
            let phy_base = entry.get_pa();
            let curr_prot = entry.get_prop();

            let mut new_leaf = Box::new(PageTable::default());
            if entry.is_valid() {
                new_leaf.iter_mut().enumerate().for_each(|(idx, elem)| {
                    *elem = PageTableEntry::new(phy_base + (idx << shift), curr_prot);
                });
            }
            let new_pte = Box::into_raw(new_leaf);
            *entry = PageTableEntry::new_leaf(virt_to_phy(VirtualAddress(new_pte as _)));
            unsafe {
                asm!("sfence.vma");
            }
        }

        unsafe { &mut *(entry.get_pa().into_virt().addr() as *mut Self) }
    }

    pub fn set_prop_range<R>(&mut self, range: R, prop: usize)
    where
        R: SliceIndex<[PageTableEntry], Output = [PageTableEntry]>,
    {
        self.entries[range].iter_mut().for_each(|elem| {
            if let Some(leaf) = elem.to_leaf_mut() {
                leaf.set_prop_range(..ENTRIES_PER_TABLE, prop);
            } else if elem.is_valid() {
                elem.set_prop(prop);
            }
        });
    }

    pub fn add_ref_count<R>(&mut self, range: R)
    where
        R: SliceIndex<[PageTableEntry], Output = [PageTableEntry]>,
    {
        self.entries[range].iter_mut().for_each(|elem| {
            if elem.is_leaf() {
                unsafe { &mut *(elem.get_pa().into_virt().addr() as *mut PageTable) }
                    .add_ref_count(..);
            } else if elem.is_valid() {
                memory_alloc::ALLOCATOR.increase_ref_count(elem.get_pa().0);
            }
        });
    }

    pub fn set_fork_prop<R>(&mut self, range: R)
    where
        R: SliceIndex<[PageTableEntry], Output = [PageTableEntry]>,
    {
        self.entries[range].iter_mut().for_each(|elem| {
            if let Some(leaf) = elem.to_leaf_mut() {
                leaf.set_fork_prop(0..leaf.entries.len());
            } else if elem.is_valid() && elem.is_set(PTE_W) {
                elem.set_prop(elem.get_prop() & (!PTE_W));
            }
        });
    }
}

pub static mut PGD: PageTable = PageTable {
    entries: [PageTableEntry(0); 512],
};

#[inline]
pub const fn vpn2(addr: VirtualAddress) -> usize {
    (addr.addr() >> PGD_SHIFT) & 0x1ff
}

#[inline]
pub const fn vpn1(addr: VirtualAddress) -> usize {
    (addr.addr() >> PMD_SHIFT) & 0x1ff
}

#[inline]
pub const fn vpn0(addr: VirtualAddress) -> usize {
    (addr.addr() >> PTE_SHIFT) & 0x1ff
}

#[inline]
pub const fn vpn(addr: VirtualAddress, shift: u32) -> usize {
    (addr.addr() >> shift) & 0x1ff
}

#[inline]
pub fn make_satp(pa: PhysicalAddress) -> usize {
    (pa.0 >> 12) | SATP_SV39
}

#[inline]
pub fn virt_to_phy(va: VirtualAddress) -> PhysicalAddress {
    PhysicalAddress(va.addr() - PAGE_OFFSET.addr() + phy_begin())
}

#[inline]
pub fn phy_to_virt(pa: PhysicalAddress) -> VirtualAddress {
    VirtualAddress(pa.addr() - phy_begin() + PAGE_OFFSET.addr())
}

#[inline]
pub fn root_pgd_clone() -> PageTable {
    let ptr = &raw const PGD;
    let mut pgd_clone = PageTable {
        entries: [PageTableEntry(0); 512],
    };
    pgd_clone.entries[256..].copy_from_slice(&unsafe { &*ptr }.entries[256..]);

    pgd_clone.set_fork_prop(0..256);
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

#[unsafe(no_mangle)]
extern "C" fn init_virtual_memory(dtb_addr: u64, kernel_start: usize, kernel_end: usize) {
    // identity mapping
    // kernel_startup mapping
    let offset = PAGE_OFFSET + 0x200000 - kernel_start;
    for i in (kernel_start & PGD_MASK..align(kernel_end, PGD_SIZE)).step_by(1 << PGD_SHIFT) {
        let va = VirtualAddress(i);
        let pa = PhysicalAddress(i);
        unsafe {
            PGD[va.vpn2()] = PageTableEntry::new(pa, PROT_KERNEL);
            PGD[(offset + va).vpn2()] = PageTableEntry::new(pa, PROT_KERNEL);
        }
    }

    // fdt mapping
    unsafe {
        PGD[(offset + dtb_addr as usize).vpn2()] =
            PageTableEntry::new(PhysicalAddress((dtb_addr as usize) & PGD_MASK), PROT_KERNEL);
    }

    unsafe {
        satp::set(satp::Mode::Sv39, 0, (&raw const PGD as usize) >> 12);
    }
    riscv::asm::sfence_vma_all();
}

#[unsafe(no_mangle)]
extern "C" fn drop_identity(kernel_start: usize, kernel_end: usize) {
    // identity mapping
    for i in (kernel_start & !((1 << PGD_SHIFT) - 1)..align(kernel_end, 1 << PGD_SHIFT))
        .step_by(1 << PGD_SHIFT)
    {
        let i = VirtualAddress(i);
        unsafe {
            PGD[i.vpn2()].clear();
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

            set_memory_prop(PhysicalAddress(phy_base), size, PROT_RD_ONLY);
        }
    }
}

pub fn set_memory_prop(phy_base: PhysicalAddress, size: usize, prop: usize) {
    extern crate alloc;
    let virt_base = VirtualAddress(phy_base.addr() - phy_begin() + PAGE_OFFSET.addr());
    // last page counted
    let virt_end = VirtualAddress(align((virt_base + size).addr(), 0x1000) - 0x1000);

    let pgd_ptr = &raw mut PGD;

    match size {
        0..PMD_SIZE => {
            let pmd = unsafe { (&mut *pgd_ptr).try_new_entry(vpn2(virt_base), PMD_SHIFT) };
            let pte = pmd.try_new_entry(vpn1(virt_base), PTE_SHIFT);
            pte.set_prop_range(vpn0(virt_base)..=vpn0(virt_end), prop);
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
                if gb_idx == vpn2(virt_base) && virt_base & (!PMD_MASK) != VirtualAddress(0) {
                    let pte = pmd.try_new_entry(vpn1(virt_base), PTE_SHIFT);
                    pte.set_prop_range(vpn0(virt_base)..ENTRIES_PER_TABLE, prop);
                    mb_start += 1;
                }

                if gb_idx == vpn2(virt_end) && virt_end & (!PMD_MASK) != VirtualAddress(0) {
                    let pte = pmd.try_new_entry(vpn1(virt_end), PTE_SHIFT);
                    pte.set_prop_range(..=vpn0(virt_end), prop);
                    mb_end -= 1;
                }

                if mb_start <= mb_end {
                    pmd.set_prop_range(mb_start..=mb_end, prop);
                }
            }
        } // PGD_SIZE.. => for gb_idx in vpn2(virt_base)..=vpn2(virt_end) {},
    }
}

pub fn io_remap(phy_base: PhysicalAddress, size: usize) -> VirtualAddress {
    let io_remap_curr = unsafe { VIRT_IO_REMAN_BEGIN };
    let aligned_phy_base = phy_base.addr() & PMD_MASK;

    let size = align(size, PMD_SIZE);
    let pgd_ptr = &raw mut PGD;

    for offset in (0..size).step_by(PMD_SIZE) {
        let offset = VirtualAddress(offset);
        let pmd_entry =
            unsafe { (*pgd_ptr).try_new_entry(vpn2(offset + io_remap_curr), PGD_SHIFT) };
        pmd_entry[vpn1(offset + io_remap_curr)] =
            PageTableEntry::new(PhysicalAddress(offset.0 + aligned_phy_base), PROT_MMIO);
    }

    unsafe { VIRT_IO_REMAN_BEGIN += size };
    VirtualAddress(io_remap_curr + (phy_base.addr() - aligned_phy_base))
}

pub fn pagewalk(root_pgd: *mut PageTable, va: VirtualAddress, pa: PhysicalAddress, prop: usize) {
    let va = va.addr() & PAGE_MASK;
    let pa = pa.addr() & PAGE_MASK;
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
        *entry = PageTableEntry::new(PhysicalAddress(pa), prop);
    }
}

pub fn find_page_entry(pgd: &PageTable, va: VirtualAddress) -> Option<&PageTableEntry> {
    let shift_offset = 9;
    let mut shift = PGD_SHIFT - shift_offset;

    let mut entry_ptr = &pgd[va.vpn2()] as *const PageTableEntry;

    while let Some(table) = unsafe { &*entry_ptr }.to_leaf_ref() {
        let next_entry = &table[vpn(va, shift)];
        shift -= shift_offset;

        entry_ptr = next_entry;
    }

    let entry = unsafe { &*entry_ptr };
    if entry.is_valid() { Some(entry) } else { None }
}

pub fn find_page_entry_mut(pgd: &mut PageTable, va: VirtualAddress) -> Option<&mut PageTableEntry> {
    let shift_offset = 9;
    let mut shift = PGD_SHIFT - shift_offset;

    let mut entry_ptr = &mut pgd[va.vpn2()] as *mut PageTableEntry;

    while let Some(table) = unsafe { &mut *entry_ptr }.to_leaf_mut() {
        let next_entry = &mut table[vpn(va, shift)];
        shift -= shift_offset;

        entry_ptr = next_entry;
    }

    let entry = unsafe { &mut *entry_ptr };
    if entry.is_valid() { Some(entry) } else { None }
}
