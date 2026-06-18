use core::alloc;
use core::fmt;
use core::ptr;

use crate::{fdt, virtual_mem};
use log::debug;

use buddy_alloc::Page;
use dynamic_alloc::DynamicAllocator;

mod buddy_alloc;
mod dynamic_alloc;
mod startup_alloc;

const PAGE_SIZE: usize = 4096;
const PAGE_MASK: usize = !(PAGE_SIZE - 1);

pub struct GlobalAllocator {
    allocator: spin::Mutex<Option<MemoryAllocator>>,
}

unsafe impl alloc::GlobalAlloc for GlobalAllocator {
    unsafe fn alloc(&self, layout: alloc::Layout) -> *mut u8 {
        let _disable_interrupt = crate::interrupt::SModeInterrupt::new();
        let mut lock = self.allocator.lock();
        if let Some(allocator) = &mut *lock {
            if let Some(ptr) = allocator.malloc(layout.size()) {
                virtual_mem::phy_to_virt(ptr) as *mut u8
            } else {
                ptr::null_mut()
            }
        } else {
            ptr::null_mut()
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: alloc::Layout) {
        let _disable_interrupt = crate::interrupt::SModeInterrupt::new();
        if let Some(allocator) = self.allocator.lock().as_mut() {
            allocator.free(virtual_mem::virt_to_phy(ptr as usize));
        }
    }
}

impl GlobalAllocator {
    pub fn init(&self, allocator: MemoryAllocator) {
        let a = &mut *self.allocator.lock();
        *a = Some(allocator);
    }

    pub fn increase_ref_count(&self, pa: usize) {
        let _disable_interrupt = crate::interrupt::SModeInterrupt::new();
        let mut lock = self.allocator.lock();
        let Some(allocator) = lock.as_mut() else {
            return;
        };
        allocator.increase_ref_count(pa);
    }
}

impl fmt::Display for GlobalAllocator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(alloc) = self.allocator.lock().as_ref() {
            write!(f, "{}", *alloc)?;
        }
        Ok(())
    }
}

#[global_allocator]
pub static ALLOCATOR: GlobalAllocator = GlobalAllocator {
    allocator: spin::Mutex::new(None),
};

#[derive(Debug)]
pub enum Error {
    FdtParseError(fdt::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct MemoryRange {
    base: usize,
    size: usize,
}

impl PartialOrd for MemoryRange {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MemoryRange {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        if self.base != other.base {
            self.base.cmp(&other.base)
        } else {
            self.size.cmp(&other.size)
        }
    }
}

impl MemoryRange {
    fn contain<T: Into<usize>>(&self, ptr: T) -> bool {
        let ptr = ptr.into();
        self.base <= ptr && self.end() > ptr
    }
    fn overlap(&self, other: &MemoryRange) -> bool {
        self.contain(other.base)
            || self.contain(other.end())
            || other.contain(self.base)
            || other.contain(self.end())
    }

    #[inline(always)]
    fn end(&self) -> usize {
        self.base + self.size
    }
}

unsafe fn read_u64_from_u32_ptr(p: *const u32) -> u64 {
    let part1 = unsafe { ptr::read(p) }.swap_bytes() as u64;
    let part2 = unsafe { ptr::read(p.add(1)) }.swap_bytes() as u64;
    part1 << 32 | part2
}

#[derive(Default)]
pub struct MemoryLayout {
    avaliable_memory: &'static mut [MemoryRange],
    reserved_memory: &'static mut [MemoryRange],
}

impl MemoryLayout {
    fn get_avaliable_memory(
        &self,
        startup_alloc: &mut startup_alloc::StartupAllocator,
    ) -> &'static mut [MemoryRange] {
        // find avaliable_memory depends on memory_layout
        let avaliable_memory_range = align(startup_alloc.curr_ptr(), align_of::<MemoryRange>());
        let mut avaliable_memory_range_count = 0usize;
        let mut reserved_memory_ind = 0usize;

        for aval_mem in self.avaliable_memory.iter() {
            let mut reserved_memory_end: usize = self.reserved_memory.len();
            for (idx, j) in self.reserved_memory[reserved_memory_ind..]
                .iter()
                .enumerate()
            {
                if j.base < aval_mem.base {
                    reserved_memory_ind += 1;
                } else if j.base > aval_mem.end() {
                    reserved_memory_end = reserved_memory_ind + idx;
                    break;
                }
            }

            let mut curr_base = aval_mem.base;
            self.reserved_memory[reserved_memory_ind..reserved_memory_end]
                .iter()
                .for_each(|reserved_memory| {
                    if curr_base >= reserved_memory.base {
                        curr_base = reserved_memory.end();
                        return;
                    }
                    let curr_ptr = startup_alloc.alloc::<MemoryRange>();

                    unsafe {
                        *curr_ptr = MemoryRange {
                            base: curr_base,
                            size: reserved_memory.base - curr_base,
                        }
                    }

                    avaliable_memory_range_count += 1;
                    curr_base = reserved_memory.end();
                });

            if curr_base < aval_mem.end() {
                let curr_ptr = startup_alloc.alloc::<MemoryRange>();
                unsafe {
                    *curr_ptr = MemoryRange {
                        base: curr_base,
                        size: aval_mem.end() - curr_base,
                    }
                }

                avaliable_memory_range_count += 1;
            }

            reserved_memory_ind = reserved_memory_end;
        }

        unsafe {
            &mut *ptr::slice_from_raw_parts_mut(
                avaliable_memory_range as *mut MemoryRange,
                avaliable_memory_range_count,
            )
        }
    }
}

pub struct MemoryAllocator {
    buddy_allocators: buddy_alloc::BuddyZone,
    dynamic_allocator: dynamic_alloc::DynamicAllocator,
}

impl core::fmt::Display for MemoryAllocator {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(f, "{}", self.buddy_allocators)?;
        write!(f, "{}", self.dynamic_allocator)?;
        Ok(())
    }
}

impl MemoryAllocator {
    pub fn malloc(&mut self, size: usize) -> Option<usize> {
        // for size greater than 4KB

        if size > 2048 {
            let temp = align(size, 4096) / 4096;
            let order = usize::BITS - (temp - 1).leading_zeros();
            let p = self.buddy_allocators.alloc_pages(order, false)?;
            Some(p.addr())
        } else {
            self.dynamic_allocator
                .malloc(&mut self.buddy_allocators.allocators[0], size)
        }
    }

    pub fn increase_ref_count(&mut self, pa: usize) {
        self.buddy_allocators.increase_pages_ref_count(pa);
    }

    pub fn free(&mut self, ptr: usize) {
        use buddy_alloc::PageState;
        if ptr == 0 {
            log::warn!("Try to free a null ptr");
            return;
        }
        let Some(page_index) = self.buddy_allocators.find(ptr & PAGE_MASK) else {
            return;
        };
        let buddy_allocator = &mut self.buddy_allocators.allocators[page_index];
        let state = buddy_allocator.pages_state(ptr);
        let index = buddy_allocator.pages_index(ptr & PAGE_MASK);
        let order = match state {
            PageState::OccupiedSlab => {
                self.dynamic_allocator.free(ptr);
                1
            }
            PageState::Occupied(o) => {
                let ptr = ptr & PAGE_MASK;
                buddy_allocator.free_pages(Page::new(page_index, ptr));
                o as isize
            }
            PageState::Avaliable(o) => o as isize,
            _ => -1,
        };

        if order > 0 {
            debug!(
                "[*] Buddy found! buddy idx: 0x{:x} for ptr 0x{:x} with order {}",
                index, ptr, order
            );
        }
    }
}

#[inline]
fn align(n: usize, b: usize) -> usize {
    let tmp = b - 1;
    (n + tmp) & !tmp
}

pub fn get_memory_layout(
    dtb_addr: *const u8,
    kernel_start: *mut u8,
    kernel_end: *mut u8,
) -> Result<(MemoryLayout, usize), fdt::Error> {
    let mut curr_heap_byte_offset = 0_usize;

    // Find avaliable memory and reserved memory
    let mut memory_node = [0usize; 16];
    let mut reserved_memory_node = [0usize; 64];
    let mut total_memory = 0usize;
    let memory_node_num = fdt::path_all_offset(dtb_addr, "/memory", &mut memory_node)?;
    let reserved_memory_node_num =
        fdt::path_all_offset(dtb_addr, "/reserved-memory/*", &mut reserved_memory_node)?;

    let mem_entry_num = memory_node[..memory_node_num]
        .iter()
        .fold(0usize, |mut ind, item| {
            let (ptr, len) = match fdt::getprop(dtb_addr, *item, "reg") {
                Ok(v) => v,
                Err(fdt::Error::Notfound) => return ind,
                Err(e) => panic!("Unexpected error {e:#?}"),
            };
            let len = len / size_of::<u64>() / 2;
            let ptr = ptr as *const u32;

            for i in 0..len {
                let mem_off = i << 2;

                let base = ((unsafe { *ptr.wrapping_add(mem_off) }.swap_bytes() as usize) << 32)
                    | (unsafe { *ptr.wrapping_add(mem_off + 1) }.swap_bytes() as usize);

                let size = ((unsafe { *ptr.wrapping_add(mem_off + 2) }.swap_bytes() as usize)
                    << 32)
                    | (unsafe { *ptr.wrapping_add(mem_off + 3) }.swap_bytes() as usize);

                let mem_range = unsafe {
                    &mut *(kernel_end.wrapping_add(curr_heap_byte_offset) as *mut MemoryRange)
                };
                *mem_range = MemoryRange { base, size };
                curr_heap_byte_offset += size_of::<MemoryRange>();
                total_memory += size;
                ind += 1
            }
            ind
        });

    let mut reserved_memory_entry_num = reserved_memory_node[..reserved_memory_node_num]
        .iter()
        .fold(0usize, |mut ind, item| {
            let (ptr, len) = match fdt::getprop(dtb_addr, *item, "reg") {
                Ok(v) => v,
                Err(fdt::Error::Notfound) => return ind,
                Err(e) => panic!("Unexpected error {e:#?}"),
            };
            let len = len / size_of::<u64>() / 2;
            let ptr = ptr as *const u32;

            for i in 0..len {
                let mem_off = i << 2;

                let base = ((unsafe { *ptr.wrapping_add(mem_off) }.swap_bytes() as usize) << 32)
                    | (unsafe { *ptr.wrapping_add(mem_off + 1) }.swap_bytes() as usize);

                let size = ((unsafe { *ptr.wrapping_add(mem_off + 2) }.swap_bytes() as usize)
                    << 32)
                    | (unsafe { *ptr.wrapping_add(mem_off + 3) }.swap_bytes() as usize);

                let mem_range = unsafe {
                    &mut *(kernel_end.wrapping_add(curr_heap_byte_offset) as *mut MemoryRange)
                };
                *mem_range = MemoryRange { base, size };
                curr_heap_byte_offset += size_of::<MemoryRange>();
                ind += 1;
            }
            ind
        });

    let offset_chosen = fdt::path_offset(dtb_addr, "/chosen", 1)?;
    if let (Ok(a), Ok(b)) = (
        fdt::getprop(dtb_addr, offset_chosen, "linux,initrd-start"),
        fdt::getprop(dtb_addr, offset_chosen, "linux,initrd-end"),
    ) {
        let base = unsafe { read_u64_from_u32_ptr(a.0 as *const u32) };
        let end = unsafe { read_u64_from_u32_ptr(b.0 as *const u32) };
        let mem_range = unsafe {
            &mut *(kernel_end.wrapping_byte_add(curr_heap_byte_offset) as *mut MemoryRange)
        };
        *mem_range = MemoryRange {
            base: base as usize,
            size: (end - base) as usize,
        };
        curr_heap_byte_offset += size_of::<MemoryRange>();
        reserved_memory_entry_num += 1;
    }

    let avaliable_memory = unsafe {
        &mut *ptr::slice_from_raw_parts_mut(kernel_end as *mut MemoryRange, mem_entry_num)
    };
    avaliable_memory.sort_unstable();
    let phy_base = avaliable_memory[0].base;

    // calculate kernel address
    let page_count = total_memory / PAGE_SIZE;
    let reserved_for_page = page_count
        * (size_of::<buddy_alloc::PageFrame>() + size_of::<buddy_alloc::FreePageNode>())
        + 2 * size_of::<buddy_alloc::PageAllocator>()
        + curr_heap_byte_offset
        + size_of::<MemoryRange>();
    let mem_range =
        unsafe { &mut *(kernel_end.wrapping_byte_add(curr_heap_byte_offset) as *mut MemoryRange) };
    *mem_range = MemoryRange {
        base: (kernel_start as usize) - virtual_mem::PAGE_OFFSET + phy_base,
        size: reserved_for_page + (kernel_end as usize) - (kernel_start as usize),
    };
    curr_heap_byte_offset += size_of::<MemoryRange>();
    reserved_memory_entry_num += 1;

    let reserved_memory = unsafe {
        &mut *ptr::slice_from_raw_parts_mut(
            kernel_end.wrapping_byte_add(mem_entry_num * size_of::<MemoryRange>())
                as *mut MemoryRange,
            reserved_memory_entry_num,
        )
    };
    reserved_memory.sort_unstable();

    let memory_layout = MemoryLayout {
        avaliable_memory,
        reserved_memory,
    };
    Ok((memory_layout, curr_heap_byte_offset))
}

pub fn init_memory_allocator(
    dtb_addr: *const u8,
    kernel_start: *mut u8,
    kernel_end: *mut u8,
) -> Result<MemoryAllocator, Error> {
    let (memory_layout, off) = match get_memory_layout(dtb_addr, kernel_start, kernel_end) {
        Ok(s) => Ok(s),
        Err(e) => Err(Error::FdtParseError(e)),
    }?;

    // Set whole memory section kernel mapping
    unsafe {
        use crate::virtual_mem::*;
        VIRT_MAP_BEGIN = memory_layout.avaliable_memory[0].base;
        VIRT_IO_REMAN_BEGIN = align(
            memory_layout.avaliable_memory.last().unwrap().end(),
            PGD_SIZE,
        ) + PAGE_OFFSET;
    }
    memory_layout.avaliable_memory.iter().for_each(|range| {
        use crate::virtual_mem::*;
        for i in (range.base & PGD_MASK..range.end()).step_by(PGD_SIZE) {
            unsafe { PGD[vpn2(phy_to_virt(i))] = PageTableEntry::new(i, PROT_KERNEL) };
        }
    });

    log::info!(
        "Avaliable Memory layout: {:#x?}",
        memory_layout.avaliable_memory
    );
    log::info!(
        "Reserved Memory layout: {:#x?}",
        memory_layout.reserved_memory
    );

    let mut startup_alloc = startup_alloc::StartupAllocator::new(
        kernel_end.wrapping_add(off) as usize,
        memory_layout.reserved_memory,
    );

    let pages = buddy_alloc::BuddyZone::new(&mut startup_alloc, &memory_layout)?;

    debug!("test logger");
    log::error!("Test logger error");

    Ok(MemoryAllocator {
        buddy_allocators: pages,
        dynamic_allocator: DynamicAllocator::default(),
    })
}
