use core::{alloc, ptr};

use super::MemoryRange;

pub struct StartupAllocator {
    base: usize,
    off: usize,
    reserved_memory: &'static [super::MemoryRange],
    reserved_memory_idx: usize,
}

impl StartupAllocator {
    pub fn new(base: usize, reserved_memory: &'static [super::MemoryRange]) -> Self {
        let mut reserved_memory_idx = 0;
        while reserved_memory_idx < reserved_memory.len()
            && reserved_memory[reserved_memory_idx].base < base
        {
            reserved_memory_idx += 1;
        }
        Self {
            base,
            off: 0,
            reserved_memory,
            reserved_memory_idx,
        }
    }

    pub fn alloc_size_align(&mut self, size: usize, align: usize) -> usize {
        self.off = crate::align(self.off, align);
        let mut alloc_range = MemoryRange {
            base: self.base + self.off,
            size,
        };
        while self.reserved_memory_idx < self.reserved_memory.len()
            && self.reserved_memory[self.reserved_memory_idx].overlap(&alloc_range)
        {
            alloc_range.base = crate::align(self.reserved_memory[self.reserved_memory_idx].end(), align);
            self.reserved_memory_idx += 1;
        }
        self.off = alloc_range.end() - self.base;
        alloc_range.base
    }

    pub fn alloc<T: Sized>(&mut self) -> *mut T {
        let layout = alloc::Layout::new::<T>();
        self.alloc_size_align(layout.size(), layout.align()) as *mut T
    }

    pub fn alloc_list<T: Sized>(&mut self, size: usize) -> *mut [T] {
        ptr::slice_from_raw_parts_mut(
            self.alloc_size_align(size_of::<T>() * size, align_of::<T>()) as *mut T,
            size,
        )
    }

    pub fn curr_ptr(&self) -> usize {
        self.base + self.off
    }
}
