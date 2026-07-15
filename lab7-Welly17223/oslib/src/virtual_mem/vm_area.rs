extern crate alloc;

use core::{alloc::Layout, fmt::Debug};

use alloc::{
    boxed::Box,
    collections::{BTreeMap, BTreeSet},
};

use crate::{
    file_system::{self, SeekFrom},
    virtual_mem::{PGD_SIZE, PTE_V, VirtualAddress},
};

use super::{PageTable, PageTableEntry};

#[derive(Default, Debug)]
pub struct Manager {
    vm_area: BTreeMap<super::VirtualAddress, AreaEntry>,
    vm_free_addr: BTreeMap<VirtualAddress, usize>,
    vm_free_size: BTreeSet<(usize, VirtualAddress)>,
    pub pgd: Box<PageTable>,
}

#[derive(Debug)]
pub enum Error {
    NotEnough,
    NotFound,
    NotMapped,
    AlreadyMapPTE { prop_xor: usize },
    Inaccessible,
}

impl From<Error> for isize {
    fn from(val: Error) -> Self {
        match val {
            Error::NotEnough => 1,
            Error::NotFound => 2,
            Error::NotMapped => 3,
            Error::AlreadyMapPTE { prop_xor: _ } => 4,
            Error::Inaccessible => 5,
        }
    }
}

#[derive(Default, Debug, Clone)]
struct AreaEntry {
    size: usize,
    flags: usize,
    backed: Provider,
}

#[derive(Clone)]
pub enum Provider {
    Anonymous,
    File(file_system::File),
    Mem(&'static [u8]),
}

impl Default for Provider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider {
    fn new() -> Self {
        Self::Anonymous
    }

    fn file(file: file_system::File) -> Self {
        Self::File(file)
    }
}

impl Debug for Provider {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let str = match self {
            Self::Anonymous => "Anonymous",
            Self::File(_) => "File",
            Self::Mem(_) => "Mem",
        };
        write!(f, "Provider::{str}")
    }
}

impl AreaEntry {
    fn new(size: usize, flags: usize, backed: Provider) -> Self {
        Self {
            size,
            flags,
            backed,
        }
    }
}

impl Manager {
    pub fn new() -> Self {
        let mut vm_free = BTreeMap::new();
        let mut vm_free_size = BTreeSet::new();
        // For 0 ~ 255 free page
        vm_free.insert(VirtualAddress(0), PGD_SIZE * super::ENTRIES_PER_TABLE / 2);
        vm_free_size.insert((PGD_SIZE * super::ENTRIES_PER_TABLE / 2, VirtualAddress(0)));

        Self {
            vm_area: BTreeMap::default(),
            vm_free_addr: vm_free,
            vm_free_size,
            pgd: Box::new(super::root_pgd_clone()),
        }
    }

    pub fn map_addr(
        &mut self,
        start: VirtualAddress,
        size: usize,
        flags: usize,
        backed: Provider,
    ) -> Result<VirtualAddress, Error> {
        let size = super::align(size, 0x1000);

        match self.vm_free_addr.range(..=start).next() {
            Some(e) if e.0.0 + e.1 >= start.0 + size => {
                let hole_base = *e.0;
                let hole_size = *e.1;
                self.remove_hole(&hole_base);

                let area = AreaEntry::new(size, flags, backed);
                self.vm_area.insert(start, area);

                let front_size = start.0 - hole_base.0;
                if front_size > 0 {
                    let front_begin = hole_base;
                    self.insert_hole(&front_begin, front_size);
                }

                let back_size = hole_base.0 + hole_size - (start.0 + size);
                if back_size > 0 {
                    let back_begin = VirtualAddress(start.0 + size);
                    self.insert_hole(&back_begin, back_size);
                }

                Ok(start)
            }
            _ => self.map(size, flags, backed),
        }
    }

    pub fn map(
        &mut self,
        size: usize,
        flags: usize,
        backed: Provider,
    ) -> Result<VirtualAddress, Error> {
        let dummy = (size, VirtualAddress(0));
        let size = crate::align(size, 0x1000);

        match self.vm_free_size.range(dummy..).next() {
            Some((hole_size, hole_base)) => {
                let hole_size = *hole_size;
                let hole_base = *hole_base;

                self.remove_hole(&hole_base);

                let area = AreaEntry::new(size, flags, backed);
                self.vm_area.insert(hole_base, area);

                let hole_size = hole_size - size;

                if hole_size > 0 {
                    let hole_base = VirtualAddress(hole_base.0 + size);
                    self.insert_hole(&hole_base, hole_size);
                }

                Ok(hole_base)
            }
            None => Err(Error::NotFound),
        }
    }

    pub fn map_file(
        &mut self,
        file: file_system::File,
        flags: usize,
    ) -> Result<VirtualAddress, Error> {
        let size = file.len().map_err(|_| Error::NotFound)? as usize;
        let backed = Provider::file(file);
        self.map(size, flags, backed)
    }

    pub fn map_file_addr(
        &mut self,
        start: VirtualAddress,
        file: file_system::File,
        flags: usize,
    ) -> Result<VirtualAddress, Error> {
        let size = file.len().map_err(|_| Error::NotFound)? as usize;
        let backed = Provider::file(file);
        self.map_addr(start, size, flags, backed)
    }

    pub fn unmap(&mut self, addr: VirtualAddress) -> Result<(), Error> {
        let area_entry = self.vm_area.remove(&addr).ok_or(Error::NotFound)?;

        let drop_pte_f: fn(&mut PageTableEntry) = match area_entry.backed {
            Provider::Anonymous => |e| {
                drop(unsafe { Box::from_raw(e.get_pa().into_virt().addr() as *mut u8) });
                e.clear()
            },
            Provider::File(_) => |e| e.clear(),
            Provider::Mem(_) => |_| {},
        };

        for i in (addr.addr()..addr.addr() + area_entry.size).step_by(0x1000) {
            if let Some(e) = self.page_entry_mut(i.into()) {
                drop_pte_f(e);
            }
        }
        let mut hole_start = addr;
        let mut hole_size = area_entry.size;

        let prev = self.vm_free_addr.range(..addr).next_back();

        if let Some((base, size)) = prev
            && *base + *size == hole_start
        {
            let prev_base = *base;
            let prev_size = self.remove_hole(&prev_base).unwrap();
            hole_start = prev_base;
            hole_size += prev_size;
        }

        let next = self.vm_free_addr.range(addr..).next();
        if let Some(next) = next
            && addr + area_entry.size == *next.0
        {
            let next_base = *next.0;
            let next_size = self.remove_hole(&next_base).unwrap();
            hole_size += next_size;
        }

        self.insert_hole(&hole_start, hole_size);
        Ok(())
    }

    pub fn map_to_phy(&mut self, addr: VirtualAddress) -> Result<(), Error> {
        let (&area_addr, area_entry) = self
            .vm_area
            .range(..=addr)
            .next_back()
            .ok_or(Error::NotFound)?;

        if area_addr.0 + area_entry.size > addr.0 {
            if let Some(entry) = self.page_entry_ref(addr) {
                let prop_xor = (entry.get_prop() ^ area_entry.flags) & 0b1110;
                return Err(Error::AlreadyMapPTE { prop_xor });
            }

            if area_entry.flags & 0b0111 == 0 {
                return Err(Error::Inaccessible);
            }

            let offset = addr.0 - area_addr.0;
            let area_entry = self.vm_area.get_mut(&area_addr).unwrap();
            let pa = match &mut area_entry.backed {
                Provider::Anonymous => {
                    VirtualAddress(Box::into_raw(Box::new([0u8; 4096])) as usize).into_phy()
                }
                Provider::File(f) => {
                    let offset_align = offset & (!0xfff);
                    let mut page = Box::new([0u8; 4096]);

                    f.seek(SeekFrom::Start(offset_align as _)).unwrap();
                    f.read(page.as_mut()).map_err(|_| Error::NotFound)?;

                    let base = Box::into_raw(page) as *const _ as *const u8;
                    VirtualAddress(base as usize).into_phy()
                }
                Provider::Mem(mem) => {
                    let offset_align = offset & (!0xfff);

                    VirtualAddress(&mem[0] as *const _ as usize + offset_align).into_phy()
                }
            };
            super::pagewalk(
                self.pgd.as_mut() as *mut _,
                addr,
                pa,
                area_entry.flags | PTE_V,
            );
            Ok(())
        } else {
            Err(Error::NotMapped)
        }
    }

    fn insert_hole(&mut self, base: &VirtualAddress, size: usize) {
        self.vm_free_addr.insert(*base, size);
        self.vm_free_size.insert((size, *base));
    }

    fn remove_hole(&mut self, base: &VirtualAddress) -> Option<usize> {
        if let Some(size) = self.vm_free_addr.remove(base) {
            self.vm_free_size.remove(&(size, *base));
            Some(size)
        } else {
            None
        }
    }

    #[inline]
    pub fn satp(&self) -> usize {
        super::make_satp(super::virt_to_phy(VirtualAddress(
            self.pgd.as_ref() as *const _ as usize,
        )))
    }

    #[inline]
    pub fn page_entry_ref(&self, va: VirtualAddress) -> Option<&PageTableEntry> {
        super::find_page_entry(&self.pgd, va)
    }

    #[inline]
    pub fn page_entry_mut(&mut self, va: VirtualAddress) -> Option<&mut PageTableEntry> {
        super::find_page_entry_mut(&mut self.pgd, va)
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        self.pgd.entries[..256].iter_mut().for_each(drop_page_entry);
    }
}

impl Clone for Manager {
    fn clone(&self) -> Self {
        let mut new_pgd = Box::new(PageTable {
            entries: self.pgd.entries,
        });

        new_pgd.entries[..256]
            .iter_mut()
            .for_each(clone_page_table_entry);

        Self {
            vm_area: self.vm_area.clone(),
            vm_free_addr: self.vm_free_addr.clone(),
            vm_free_size: self.vm_free_size.clone(),
            pgd: new_pgd,
        }
    }
}

fn clone_page_table(page_table_slice: &PageTable) -> PageTable {
    let mut new_entry = page_table_slice.entries;
    new_entry.iter_mut().for_each(clone_page_table_entry);
    PageTable { entries: new_entry }
}

fn clone_page_table_entry(elem: &mut PageTableEntry) {
    if let Some(leaf) = elem.to_leaf_mut() {
        let new_elem = Box::from(clone_page_table(leaf));
        *elem = PageTableEntry::new(
            VirtualAddress(Box::into_raw(new_elem) as usize).into_phy(),
            PTE_V,
        );
    } else if elem.is_valid() {
        crate::memory_alloc::ALLOCATOR.increase_ref_count(elem.get_pa().0);
    }
}

fn drop_pagetable_inner(pagetable: &mut PageTable) {
    pagetable.entries.iter_mut().for_each(drop_page_entry);
}

fn drop_page_entry(elem: &mut PageTableEntry) {
    if let Some(leaf) = elem.to_leaf_mut() {
        drop_pagetable_inner(leaf);
        drop(unsafe { Box::from_raw(leaf as _) })
    } else if elem.is_valid() {
        unsafe {
            alloc::alloc::dealloc(
                elem.get_pa().into_virt().addr() as _,
                Layout::new::<PageTableEntry>(),
            );
        }
    }
}
