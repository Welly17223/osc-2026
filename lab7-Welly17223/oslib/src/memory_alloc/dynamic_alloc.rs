use log::debug;

use super::{
    PAGE_MASK,
    buddy_alloc::{self, Page, PageAllocator},
};
use core::fmt;

#[derive(Debug, Clone, Copy)]
enum Type {
    Size16,
    Size32,
    Size64,
    Size128,
    Size256,
    Size512,
    Size1024,
    Size2048,
}

impl From<Type> for usize {
    fn from(val: Type) -> Self {
        match val {
            Type::Size16 => 16,
            Type::Size32 => 32,
            Type::Size64 => 64,
            Type::Size128 => 128,
            Type::Size256 => 256,
            Type::Size512 => 512,
            Type::Size1024 => 1024,
            Type::Size2048 => 2048,
        }
    }
}

impl Type {
    fn from_index(order: usize) -> Option<Self> {
        match order {
            0 => Some(Self::Size16),
            1 => Some(Self::Size32),
            2 => Some(Self::Size64),
            3 => Some(Self::Size128),
            4 => Some(Self::Size256),
            5 => Some(Self::Size512),
            6 => Some(Self::Size1024),
            7 => Some(Self::Size2048),
            _ => None,
        }
    }
}

#[derive(Debug, Default)]
pub struct DynamicAllocator {
    // From 2048 to 16 Bytes
    pub dynamic_allocator: [Option<usize>; 9],
}

#[repr(C)]
struct DynamicAllocatorHeader {
    class: Type,
    memory_header: usize,
    total_entry: usize,
    fragment_page: Page,
    remain_item: [u8; 33],
}

#[derive(Debug)]
#[repr(C)]
struct DynamicAllocatorHeader32_2048Bytes {
    class: Type,
    memory_header: usize,
    total_entry: usize,
    fragment_page: Page,
    used_entry: u128,
    next_same_size_allocator: usize,
    prev_same_size_allocator: usize,
}

#[derive(Debug)]
#[repr(C)]
struct DynamicAllocatorHeader_16Bytes {
    class: Type,
    memory_header: usize,
    total_entry: usize,
    fragment_page: buddy_alloc::Page,
    used_entry: [u128; 2],
    next_same_size_allocator: usize,
    prev_same_size_allocator: usize,
}

impl fmt::Display for DynamicAllocatorHeader32_2048Bytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "DynamicAllocatorHeader class: {:?}", self.class)?;
        writeln!(f, "\tmemory header: {:x}", self.memory_header)?;
        writeln!(f, "\ttotal_entry: {:x}", self.total_entry)?;
        writeln!(f, "\tfragment_page: {:x}", self.fragment_page.addr())?;
        writeln!(f, "\tused_entry: 0b{:x}", self.used_entry)?;
        Ok(())
    }
}

impl fmt::Display for DynamicAllocatorHeader_16Bytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "DynamicAllocatorHeader class: {:?}", self.class)?;
        writeln!(f, "\tmemory header: {:x}", self.memory_header)?;
        writeln!(f, "\ttotal_entry: {:x}", self.total_entry)?;
        writeln!(f, "\tfragment_page: {:x}", self.fragment_page.addr())?;
        writeln!(
            f,
            "\tused_entry: 0x{:x}{:x}",
            self.used_entry[1], self.used_entry[0]
        )?;
        Ok(())
    }
}

#[allow(unused)]
trait DynamicAllocatorTrait {
    fn is_full(&self) -> bool;
    fn is_empty(&self) -> bool;
    fn alloc(&mut self) -> Option<usize>;
    fn free(&mut self, address: usize);
    fn frame_size(&self) -> usize;
}

impl DynamicAllocatorTrait for DynamicAllocatorHeader32_2048Bytes {
    fn is_full(&self) -> bool {
        self.used_entry.trailing_ones() as usize == self.total_entry
    }

    fn is_empty(&self) -> bool {
        self.used_entry == 0
    }

    fn alloc(&mut self) -> Option<usize> {
        let first_avaliable = self.used_entry.trailing_ones() as usize;
        if first_avaliable >= self.total_entry {
            return None;
        }

        debug!(
            "[Chunk] Allocate 0x{:x} at chunk size {}",
            self.memory_header + (16 * first_avaliable),
            usize::from(self.class)
        );
        self.used_entry |= 1 << first_avaliable;
        Some(self.memory_header + (self.frame_size() * first_avaliable))
    }

    fn free(&mut self, address: usize) {
        let index = (address - self.memory_header) / self.frame_size();
        if index > self.total_entry {
            return;
        }
        debug!(
            "[Chunk] Free 0x{:x} at chunk size {}",
            address,
            usize::from(self.class)
        );

        self.used_entry &= !(1 << index);
    }

    fn frame_size(&self) -> usize {
        self.class.into()
    }
}

impl DynamicAllocatorHeader32_2048Bytes {
    pub fn new(p: Page, class: Type, prev: usize, next: usize) -> Self {
        let frame_size: usize = class.into();
        DynamicAllocatorHeader32_2048Bytes {
            class,
            memory_header: crate::align(p.addr() + size_of::<Self>(), frame_size),
            total_entry: (super::PAGE_SIZE - size_of::<Self>()) / frame_size,
            fragment_page: p,
            used_entry: 0,
            next_same_size_allocator: next,
            prev_same_size_allocator: prev,
        }
    }
}

impl DynamicAllocatorTrait for DynamicAllocatorHeader_16Bytes {
    fn frame_size(&self) -> usize {
        self.class.into()
    }
    fn is_full(&self) -> bool {
        self.used_entry
            .iter()
            .fold(0usize, |curr, i| curr + i.trailing_ones() as usize)
            == self.total_entry
    }

    fn is_empty(&self) -> bool {
        self.used_entry.iter().all(|i| *i == 0)
    }

    fn alloc(&mut self) -> Option<usize> {
        let mut first_avaliable = -1_isize;
        let mut index = -1_isize;

        for (ind, i) in self.used_entry.iter().enumerate() {
            if !i == 0 {
                continue;
            }

            if index == -1 {
                index = ind as isize;
                first_avaliable = i.trailing_ones() as isize;
                break;
            }
        }

        if first_avaliable == -1 {
            return None;
        }

        let first_avaliable = first_avaliable as usize;
        let index = index as usize;

        if first_avaliable >= self.total_entry {
            return None;
        }
        debug!(
            "[Chunk] Allocate 0x{:x} at chunk size {}",
            self.memory_header + (16 * first_avaliable),
            usize::from(self.class)
        );

        self.used_entry[index] |= 1 << first_avaliable;
        Some(self.memory_header + (16 * first_avaliable))
    }

    fn free(&mut self, address: usize) {
        let index = (address - self.memory_header) / self.frame_size();
        if index > self.total_entry {
            return;
        }
        debug!(
            "[Chunk] Free 0x{:x} at chunk size {}",
            address,
            usize::from(self.class)
        );

        self.used_entry[index >> 7] &= !(1 << (index & (128 - 1)));
    }
}

impl DynamicAllocatorHeader_16Bytes {
    pub fn new(p: Page, prev: usize, next: usize) -> Self {
        DynamicAllocatorHeader_16Bytes {
            class: Type::Size16,
            memory_header: crate::align(p.addr() + size_of::<Self>(), 16),
            total_entry: (super::PAGE_SIZE - size_of::<Self>()) / 16,
            fragment_page: p,
            used_entry: [0; 2],
            next_same_size_allocator: next,
            prev_same_size_allocator: prev,
        }
    }
}

impl DynamicAllocator {
    pub fn malloc(&mut self, page_allocator: &mut PageAllocator, size: usize) -> Option<usize> {
        if size == 0 {
            return None;
        }

        // 16B is the first element in the list
        let rounded_exp = match size {
            s if s <= 16 => 0,
            s if s.is_power_of_two() => s.ilog2() - 4,
            s => s.ilog2() - 3,
        } as usize;

        match self.dynamic_allocator[rounded_exp] {
            Some(head_ptr) if rounded_exp != 0 => {
                let mut meta_data =
                    unsafe { &mut *(head_ptr as *mut DynamicAllocatorHeader32_2048Bytes) };

                while meta_data.next_same_size_allocator != head_ptr {
                    if !meta_data.is_full() {
                        return meta_data.alloc();
                    }
                    meta_data = unsafe {
                        &mut *(meta_data.next_same_size_allocator
                            as *mut DynamicAllocatorHeader32_2048Bytes)
                    };
                }

                if !meta_data.is_full() {
                    return meta_data.alloc();
                }

                // let alloc_page = if rounded_exp >= 7 { rounded_exp - 6 } else { 1 } as u32;
                let head_meta_data =
                    unsafe { &mut *(head_ptr as *mut DynamicAllocatorHeader32_2048Bytes) };
                let head_meta_data_prev = unsafe {
                    &mut *(head_meta_data.prev_same_size_allocator
                        as *mut DynamicAllocatorHeader32_2048Bytes)
                };
                let p = page_allocator.alloc_pages(0, true)?;
                head_meta_data.prev_same_size_allocator = p.virt_addr();
                head_meta_data_prev.next_same_size_allocator = p.virt_addr();

                let meta_data =
                    unsafe { &mut *(p.virt_addr() as *mut DynamicAllocatorHeader32_2048Bytes) };

                *meta_data = DynamicAllocatorHeader32_2048Bytes::new(
                    p,
                    Type::from_index(rounded_exp)?,
                    head_meta_data.prev_same_size_allocator,
                    head_ptr,
                );

                meta_data.alloc()
            }
            Some(head_ptr) => {
                let mut meta_data =
                    unsafe { &mut *(head_ptr as *mut DynamicAllocatorHeader_16Bytes) };

                while meta_data.next_same_size_allocator != head_ptr {
                    if !meta_data.is_full() {
                        return meta_data.alloc();
                    }
                    meta_data = unsafe {
                        &mut *(meta_data.next_same_size_allocator
                            as *mut DynamicAllocatorHeader_16Bytes)
                    };
                }

                if !meta_data.is_full() {
                    return meta_data.alloc();
                }

                let head_meta_data =
                    unsafe { &mut *(head_ptr as *mut DynamicAllocatorHeader32_2048Bytes) };
                let head_meta_data_prev = unsafe {
                    &mut *(head_meta_data.prev_same_size_allocator
                        as *mut DynamicAllocatorHeader32_2048Bytes)
                };

                let p = page_allocator.alloc_pages(0, true)?;
                head_meta_data.prev_same_size_allocator = p.virt_addr();
                head_meta_data_prev.next_same_size_allocator = p.virt_addr();

                let meta_data =
                    unsafe { &mut *(p.virt_addr() as *mut DynamicAllocatorHeader_16Bytes) };

                *meta_data = DynamicAllocatorHeader_16Bytes::new(
                    p,
                    head_meta_data.prev_same_size_allocator,
                    head_ptr,
                );

                meta_data.alloc()
            }
            None if rounded_exp != 0 => {
                let p = page_allocator.alloc_pages(0, true)?;
                self.dynamic_allocator[rounded_exp] = Some(p.virt_addr());

                let head_addr = p.virt_addr();
                let meta_data =
                    unsafe { &mut *(head_addr as *mut DynamicAllocatorHeader32_2048Bytes) };

                *meta_data = DynamicAllocatorHeader32_2048Bytes::new(
                    p,
                    Type::from_index(rounded_exp)?,
                    head_addr,
                    head_addr,
                );

                meta_data.alloc()
            }
            _ => {
                let p = page_allocator.alloc_pages(0, true)?;
                self.dynamic_allocator[rounded_exp] = Some(p.virt_addr());

                let head_addr = p.virt_addr();
                let meta_data =
                    unsafe { &mut *(p.virt_addr() as *mut DynamicAllocatorHeader_16Bytes) };
                *meta_data = DynamicAllocatorHeader_16Bytes::new(p, head_addr, head_addr);

                meta_data.alloc()
            }
        }
    }

    pub fn free(&mut self, ptr: usize) {
        use crate::virtual_mem;
        let virt_ptr = virtual_mem::phy_to_virt(virtual_mem::PhysicalAddress(ptr)).addr();
        let header = unsafe { &*((virt_ptr & PAGE_MASK) as *mut DynamicAllocatorHeader) };

        match header.class {
            Type::Size16 => {
                let header = unsafe {
                    &mut *((virt_ptr & PAGE_MASK) as *mut DynamicAllocatorHeader_16Bytes)
                };
                header.free(ptr);
            }
            _ => {
                let header = unsafe {
                    &mut *((virt_ptr & PAGE_MASK) as *mut DynamicAllocatorHeader32_2048Bytes)
                };
                header.free(ptr);
            }
        }
    }
}

impl fmt::Display for DynamicAllocator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.dynamic_allocator
            .iter()
            .try_for_each(|ac| -> fmt::Result {
                let Some(x) = ac else {
                    return Ok(());
                };
                let ptr = (*x) as *const DynamicAllocatorHeader;
                match unsafe { &*ptr }.class {
                    Type::Size16 => {
                        let mut idx = 0usize;
                        let mut node = unsafe { &*(ptr as *const DynamicAllocatorHeader_16Bytes) };
                        while node.next_same_size_allocator != *x {
                            writeln!(f, "idx 0x{:x}: {}", idx, node)?;
                            node = unsafe {
                                &*(node.next_same_size_allocator
                                    as *const DynamicAllocatorHeader_16Bytes)
                            };
                            idx += 1;
                        }
                        writeln!(f, "idx 0x{:x}: {}", idx, node)?;
                    }
                    _ => {
                        let mut idx = 0usize;
                        let mut node =
                            unsafe { &*(ptr as *const DynamicAllocatorHeader32_2048Bytes) };
                        while node.next_same_size_allocator != *x {
                            writeln!(f, "idx 0x{:x}: {}", idx, node)?;
                            node = unsafe {
                                &*(node.next_same_size_allocator
                                    as *const DynamicAllocatorHeader32_2048Bytes)
                            };
                            idx += 1;
                        }
                        writeln!(f, "idx 0x{:x}: {}", idx, node)?;
                    }
                }
                Ok(())
            })?;
        Ok(())
    }
}
