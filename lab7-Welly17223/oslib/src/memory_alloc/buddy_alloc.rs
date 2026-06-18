use core::{
    fmt::{self},
    ptr,
    sync::atomic,
};

use log::{debug, info};

use super::{MemoryLayout, MemoryRange, PAGE_MASK, startup_alloc};
use crate::virtual_mem;

pub struct BuddyZone {
    pub allocators: &'static mut [PageAllocator],
    allocator_status_bit: u64,
    memory_layout: MemoryLayout,
}

pub struct PageAllocator {
    pages: &'static mut [PageFrame],
    max_order: u32,
    range: MemoryRange,
    free_lists: [FreePageList; 32],
    free_list_pool: &'static mut [FreePageNode],
}

impl fmt::Display for PageAllocator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "page_base: 0x{:x}", self.range.base)?;
        writeln!(f, "page_len: {}", self.pages.len())?;
        writeln!(f, "Max order: {}", self.max_order)?;
        writeln!(f, "avaliable memory: ")?;
        for (idx, i) in self.free_lists.iter().enumerate() {
            match i.header {
                None => continue,
                Some(mut header) => {
                    write!(f, "free at order 0x{:x}: ", idx)?;

                    loop {
                        let header_ptr = header as *const FreePageNode;
                        write!(f, "{} ", unsafe { *header_ptr })?;
                        match unsafe { *header_ptr }.next {
                            None => break,
                            Some(n) => header = n,
                        }
                    }
                }
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

#[inline]
fn pair_order(num: u32, order: u32) -> u32 {
    if num & ((1 << order) - 1) != 0 {
        panic!("pair_order: 0x{num:x} {order} bits must be zero");
    } else {
        num ^ (1 << order)
    }
}

impl BuddyZone {
    pub fn new(
        startup_alloc: &mut startup_alloc::StartupAllocator,
        memory_layout: &MemoryLayout,
    ) -> Result<Self, super::Error> {
        let page_allocator = unsafe {
            &mut *startup_alloc.alloc_list::<PageAllocator>(memory_layout.avaliable_memory.len())
        };

        let mut reserved_start = 0;
        for (idx, r) in memory_layout.avaliable_memory.iter().enumerate() {
            let reserved_end = memory_layout
                .reserved_memory
                .partition_point(|reserved| reserved.base < r.end());
            while reserved_start < memory_layout.reserved_memory.len() {
                if r.overlap(&memory_layout.reserved_memory[reserved_start]) {
                    break;
                }
                reserved_start += 1;
            }

            info!(
                "reserved_start: {}, reserved_end: {}, memory_layout.reserved_memory.len(): {}, r.end: 0x{:x}",
                reserved_start,
                reserved_end,
                memory_layout.reserved_memory.len(),
                r.end()
            );

            if reserved_start < memory_layout.reserved_memory.len() {
                page_allocator[idx] = PageAllocator::new(
                    startup_alloc,
                    &memory_layout.reserved_memory[reserved_start..reserved_end],
                    *r,
                )?;
            } else {
                page_allocator[idx] = PageAllocator::new(startup_alloc, &[], *r)?;
            }

            reserved_start = reserved_end;
        }

        let memory_layout = MemoryLayout {
            avaliable_memory: unsafe {
                &mut *ptr::slice_from_raw_parts_mut(
                    memory_layout.avaliable_memory.as_ptr() as *mut MemoryRange,
                    memory_layout.avaliable_memory.len(),
                )
            },
            reserved_memory: unsafe {
                &mut *ptr::slice_from_raw_parts_mut(
                    memory_layout.reserved_memory.as_ptr() as *mut MemoryRange,
                    memory_layout.reserved_memory.len(),
                )
            },
        };

        Ok(BuddyZone {
            allocators: page_allocator,
            allocator_status_bit: 0,
            memory_layout,
        })
    }

    pub fn find(&self, mem_addr: usize) -> Option<usize> {
        for (idx, alloc) in self.allocators.iter().enumerate() {
            if alloc.range.contain(mem_addr) {
                return Some(idx);
            }
        }
        None
    }

    pub fn alloc_pages(&mut self, order: u32, is_slab: bool) -> Option<Page> {
        for i in self.allocators.iter_mut() {
            let r = i.alloc_pages(order, is_slab);
            if r.is_some() {
                return r;
            }
        }
        None
    }

    pub fn free_pages(&mut self, page: Page) {
        for i in self.allocators.iter_mut() {
            if i.range.contain(page.base_addr) {
                i.free_pages(page);
                return;
            }
        }
    }

    pub fn pages_index(&self, ptr: usize) -> Option<usize> {
        let idx = self.find(ptr)?;
        Some(self.allocators[idx].pages_index(ptr))
    }

    pub fn pages_state(&self, ptr: usize) -> Option<PageState> {
        let idx = self.find(ptr)?;
        Some(self.allocators[idx].pages_state(ptr))
    }

    pub fn increase_pages_ref_count(&mut self, ptr: usize) {
        if let Some(idx) = self.find(ptr) {
            self.allocators[idx].increase_ref_count(ptr)
        }
    }
}

impl fmt::Display for BuddyZone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "avaliable_memory: {:#x?}",
            self.memory_layout.avaliable_memory
        )?;
        write!(
            f,
            "reserved_memory: {:#x?}",
            self.memory_layout.reserved_memory
        )?;
        for p in self.allocators.iter() {
            write!(f, "{}", p)?;
        }
        Ok(())
    }
}

impl PageAllocator {
    pub fn alloc_pages(&mut self, order: u32, is_slab: bool) -> Option<Page> {
        if order > self.max_order {
            return None;
        }
        let order = order as usize;

        if self.free_lists[order].header.is_none() {
            let mut curr_order = order;
            while curr_order < self.free_lists.len() && self.free_lists[curr_order].size == 0 {
                curr_order += 1;
            }

            if curr_order >= self.free_lists.len() {
                return None;
            }

            while curr_order > order {
                let node =
                    unsafe { &*(self.free_lists[curr_order].header? as *const FreePageNode) };
                debug!(
                    "[-] Remove page 0x{:x} from order {}. Range of pages: [0x{:x}, 0x{:x}]",
                    node.page_index,
                    curr_order,
                    node.page_index,
                    node.page_index + (1 << curr_order) - 1
                );
                self.free_lists[curr_order].pop_head();

                let pair_idx = pair_order(node.page_index as u32, (curr_order - 1) as u32) as usize;
                self.pages[node.page_index].state = PageState::Avaliable(curr_order - 1);
                self.pages[pair_idx].state = PageState::Avaliable(curr_order - 1);

                debug!(
                    "[+] Add page 0x{:x} to order 0x{:x}. Range of pages: [0x{:x}, 0x{:x}]",
                    node.page_index,
                    curr_order - 1,
                    node.page_index,
                    node.page_index + (1 << (curr_order - 1)) - 1
                );
                debug!(
                    "[+] Add page 0x{:x} to order {}. Range of pages: [0x{:x}, 0x{:x}]",
                    pair_idx,
                    curr_order - 1,
                    pair_idx,
                    pair_idx + (1 << (curr_order - 1)) - 1
                );

                let origian_node = &mut self.free_list_pool[node.page_index];
                *origian_node = FreePageNode::new(node.page_index);
                self.free_lists[curr_order - 1]
                    .insert_from_ptr(origian_node as *const FreePageNode);

                let pair_node = &mut self.free_list_pool[pair_idx];
                *pair_node = FreePageNode::new(pair_idx);
                self.free_lists[curr_order - 1].insert_from_ptr(pair_node as *const FreePageNode);

                curr_order -= 1;
            }
        }

        let node = self.free_lists[order].header?;

        let page_index = unsafe { &*(node as *const FreePageNode) }.page_index;
        let page_frame = &mut self.pages[page_index];
        page_frame.state = if is_slab {
            PageState::OccupiedSlab
        } else {
            PageState::Occupied(order)
        };
        page_frame.ref_count.fetch_add(1, atomic::Ordering::Relaxed);

        debug!(
            "[-] Remove page 0x{:x} from order 0x{:x}. Range of pages: [0x{:x}, 0x{:x}]",
            page_index,
            order,
            page_index,
            page_index + (1 << order) - 1
        );
        self.free_lists[order].pop_head();
        debug!(
            "[Page] Allocate 0x{:x} at order {}, page 0x{:x}. Next address at order 1: {:x?}",
            page_frame.base_addr, order, page_index, self.free_lists[order].header
        );
        Some(Page {
            index: page_index,
            base_addr: page_frame.base_addr,
        })
    }

    #[inline]
    pub fn pages_index(&self, ptr: usize) -> usize {
        let ptr = ptr & PAGE_MASK;
        (ptr - self.range.base) >> 12
    }

    #[inline]
    pub fn increase_ref_count(&mut self, ptr: usize) {
        self.pages[self.pages_index(ptr)]
            .ref_count
            .fetch_add(1, atomic::Ordering::Relaxed);
    }

    pub fn pages_state(&self, ptr: usize) -> PageState {
        self.pages[self.pages_index(ptr)].state
    }

    pub fn free_pages(&mut self, page: Page) {
        let page_frame = &mut self.pages[page.index];
        let PageState::Occupied(mut order) = page_frame.state else {
            return;
        };

        page_frame.ref_count.fetch_sub(1, atomic::Ordering::Relaxed);
        if page_frame.ref_count.load(atomic::Ordering::Relaxed) >= 1 {
            return;
        }

        let origional_order = order;
        page_frame.state = PageState::Avaliable(order);
        let page_node = &mut self.free_list_pool[page.index];

        let pair_index = pair_order(page.index as u32, order as u32) as usize;

        if pair_index >= self.pages.len() {
            self.pages[page.index].state = PageState::Avaliable(order);
            *page_node = FreePageNode::new(page.index);
            self.free_lists[order].insert_from_ptr(page_node as *mut FreePageNode);
            debug!(
                "[+] Add page 0x{:x} to order {}. Range of pages: [0x{:x}, 0x{:x}]",
                page.index,
                order,
                page.index,
                page.index + (1 << (order)) - 1
            );

            debug!(
                "[Page] Free 0x{:x} and add back to order {}, page 0x{:x}. Next address at order {}: {:?}",
                1 << origional_order,
                order,
                page.index,
                order,
                self.free_lists[order].header
            );

            return;
        }

        let pair_frame = &mut self.pages[pair_index];
        match pair_frame.state {
            PageState::Avaliable(pair_order) if pair_order == order => (),
            _ => {
                debug!(
                    "[+] Add page 0x{:x} to order {}. Range of pages: [0x{:x}, 0x{:x}]",
                    page.index,
                    order,
                    page.index,
                    page.index + (1 << (order)) - 1
                );
                debug!(
                    "[Page] Free 0x{:x} and add back to order {}, page 0x{:x}. Next address at order {}: {:?}",
                    1 << origional_order,
                    order,
                    page.index,
                    order,
                    self.free_lists[order].header
                );
                self.pages[page.index].state = PageState::Avaliable(order);
                *page_node = FreePageNode::new(page.index);
                self.free_lists[order].insert_from_ptr(page_node as *mut FreePageNode);
                return;
            }
        }

        // Continued combine pages
        page_node.page_index = 0;
        let mut next_base = page.index;
        let mut curr_base = next_base;
        while (order as u32) < self.max_order {
            let base_index = next_base & !((1 << (order + 1)) - 1);
            let pair_index = pair_order(base_index as u32, order as u32) as usize;

            if pair_index >= self.pages.len() || base_index >= self.pages.len() {
                break;
            }

            match (&self.pages[base_index].state, &self.pages[pair_index].state) {
                (PageState::Avaliable(o1), PageState::Avaliable(o2))
                    if *o1 == order && *o1 == *o2 => {}
                _ => break,
            }

            let pair_node = &self.free_list_pool[pair_index];

            let remove_node = if pair_node.page_index != 0 {
                &mut self.free_list_pool[pair_index]
            } else {
                &mut self.free_list_pool[base_index]
            };

            if (remove_node as *const FreePageNode as usize)
                == self.free_lists[order].header.unwrap()
            {
                self.free_lists[order].pop_head();
            } else {
                if remove_node as *const _ as usize == self.free_lists[order].tail.unwrap() {
                    self.free_lists[order].tail = remove_node.prev;
                }
                self.free_lists[order].size -= 1;
                let n = remove_node.prev.unwrap();
                let prev_node = unsafe { &mut *(n as *mut FreePageNode) };
                prev_node.next = remove_node.next;
            }
            debug!(
                "[-] Remove page 0x{:x} from order {}. Range of pages: [0x{:x}, 0x{:x}]",
                remove_node.page_index,
                order,
                remove_node.page_index,
                remove_node.page_index + (1 << order) - 1
            );
            remove_node.next = None;

            order += 1;
            self.pages[pair_index].state = PageState::BuddyOf;
            self.pages[base_index].state = PageState::Avaliable(order);
            self.free_list_pool[base_index] = FreePageNode::default();

            curr_base = base_index;
            next_base &= !((1 << order) - 1);
        }

        debug!(
            "[+] Add page 0x{:x} to order {}. Range of pages: [0x{:x}, 0x{:x}]",
            curr_base,
            order,
            curr_base,
            curr_base + (1 << (order - 1)) - 1
        );

        debug!(
            "[Page] Free 0x{:x} and add back to order {}, page 0x{:x}. Next address at order {}: {:?}",
            1 << origional_order,
            order,
            page.index,
            order,
            self.free_lists[order].header
        );

        let base_node = &mut self.free_list_pool[curr_base];
        *base_node = FreePageNode::new(curr_base);
        self.free_lists[order].insert_from_ptr(base_node as *mut FreePageNode);
    }

    pub fn new(
        startup_alloc: &mut startup_alloc::StartupAllocator,
        reserved_memory: &[super::MemoryRange],
        range: MemoryRange,
    ) -> Result<PageAllocator, super::Error> {
        const PAGE_SIZE: usize = 1 << 12;

        // Calculate page numbers and inialize page frames
        let pages = unsafe { &mut *startup_alloc.alloc_list::<PageFrame>(range.size / PAGE_SIZE) };
        let page_count = pages.len();
        let max_order = (range.size / PAGE_SIZE).ilog2();

        // create pages in continue memory and reserved avaliable memory
        let mut memory_page_offset = 0;

        for i in pages.iter_mut() {
            *i = PageFrame {
                base_addr: range.base + memory_page_offset,
                state: PageState::BuddyOf,
                ref_count: atomic::AtomicU32::new(0),
            };
            memory_page_offset += PAGE_SIZE;
        }

        // Initialize free list
        let free_list_pool = unsafe { &mut *startup_alloc.alloc_list::<FreePageNode>(page_count) };
        free_list_pool
            .iter_mut()
            .for_each(|n| *n = FreePageNode::new(0));

        let mut free_lists = [FreePageList::default(); 32];

        let mut curr_pages_init_count = 0usize;
        let mut res_mem_idx = 0usize;

        log::trace!("page count: {:#x}", page_count);
        'find_free_list: while curr_pages_init_count < page_count {
            let mut curr_order = if curr_pages_init_count > 0 {
                curr_pages_init_count.trailing_zeros()
            } else {
                max_order
            };
            let mut next_idx = curr_pages_init_count + (1_usize << curr_order);
            let mut curr_range = MemoryRange {
                base: range.base + (curr_pages_init_count << 12),
                size: PAGE_SIZE << curr_order,
            };

            while res_mem_idx < reserved_memory.len()
                && curr_range.overlap(&reserved_memory[res_mem_idx])
            {
                if curr_order == 0 {
                    next_idx =
                        crate::align(reserved_memory[res_mem_idx].end() - range.base, PAGE_SIZE)
                            >> 12;

                    log::trace!(
                        "Reserved memory from page range [0x{:x}, 0x{:x}]",
                        curr_pages_init_count,
                        next_idx - 1
                    );

                    res_mem_idx += 1;
                    curr_pages_init_count = next_idx;

                    continue 'find_free_list;
                }

                curr_range.size >>= 1;
                curr_order -= 1;
                next_idx = curr_pages_init_count + (1_usize << curr_order);
            }

            while curr_range.end() > range.end() {
                curr_range.size >>= 1;
                curr_order -= 1;
                next_idx = curr_pages_init_count + (1_usize << curr_order);
            }
            log::trace!(
                "curr order: {}, curr page init count: {:#x}, next_idx: {:#x}",
                curr_order,
                curr_pages_init_count,
                next_idx
            );

            free_list_pool[curr_pages_init_count] = FreePageNode::new(curr_pages_init_count);
            free_lists[curr_order as usize]
                .insert_from_ptr((&free_list_pool[curr_pages_init_count]) as *const FreePageNode);
            pages[curr_pages_init_count].state = PageState::Avaliable(curr_order as usize);
            pages[(curr_pages_init_count + 1)..next_idx]
                .iter_mut()
                .for_each(|curr_page| {
                    curr_page.state = PageState::BuddyOf;
                });

            log::trace!("curr_pages_init_count: {:#x}", curr_pages_init_count);
            curr_pages_init_count = next_idx;
        }

        log::info!("final heap address: 0x{:x}", startup_alloc.curr_ptr());

        Ok(PageAllocator {
            pages,
            free_lists,
            range,
            free_list_pool,
            max_order,
        })
    }
}

#[derive(Debug)]
pub struct Page {
    index: usize,
    base_addr: usize,
}

impl Page {
    pub fn addr(&self) -> usize {
        self.base_addr
    }

    pub fn virt_addr(&self) -> usize {
        virtual_mem::phy_to_virt(self.base_addr)
    }

    pub fn new(index: usize, base_addr: usize) -> Self {
        Page { index, base_addr }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum PageState {
    Avaliable(usize),
    BuddyOf,
    Occupied(usize),
    OccupiedSlab,
}

pub struct PageFrame {
    state: PageState,
    base_addr: usize,
    ref_count: atomic::AtomicU32,
}

#[derive(Default, Clone, Copy)]
struct FreePageList {
    size: usize,
    header: Option<usize>,
    tail: Option<usize>,
}

#[derive(Debug, Default, PartialEq, Clone, Copy)]
pub struct FreePageNode {
    page_index: usize,
    prev: Option<usize>,
    next: Option<usize>,
}

impl fmt::Display for FreePageNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "index: 0x{:x} ", self.page_index)?;
        if let Some(s) = self.next {
            write!(f, "next: 0x{:x}", s)?;
        } else {
            write!(f, "next: None")?;
        }
        write!(f, " ")?;

        if let Some(s) = self.prev {
            write!(f, "prev: 0x{:x}", s)?;
        } else {
            write!(f, "prev: None")?;
        }
        Ok(())
    }
}

impl FreePageNode {
    pub fn end(&'static mut self) -> &'static mut FreePageNode {
        let mut ptr = self as *mut Self;

        while let Some(next) = unsafe { &*ptr }.next {
            ptr = next as *mut FreePageNode;
        }

        unsafe { &mut *ptr }
    }

    pub fn len(&self) -> usize {
        let mut ptr = self as *const Self;
        let mut len = 1;

        while let Some(next) = unsafe { &*ptr }.next {
            ptr = next as *mut FreePageNode;
            len += 1;
        }
        len
    }

    pub fn new(ind: usize) -> FreePageNode {
        FreePageNode {
            page_index: ind,
            prev: None,
            next: None,
        }
    }
}

impl FreePageList {
    pub fn insert_from_ptr(&mut self, node: *const FreePageNode) {
        if node.is_null() || !node.is_aligned() {
            return;
        }
        let node = node as *mut FreePageNode;
        self.insert(unsafe { &mut *node });
    }

    pub fn insert(&mut self, node: &'static mut FreePageNode) {
        match self.tail {
            Some(t) => {
                let tail = unsafe { &mut *(t as *mut FreePageNode) };
                node.prev = Some(t);
                tail.next = Some((node as *const FreePageNode) as usize);
            }
            None => {
                self.header = Some((node as *mut FreePageNode) as usize);
            }
        }
        self.size += node.len();
        self.tail = Some((node.end() as *mut FreePageNode) as usize);
    }

    pub fn pop_head(&mut self) {
        let Some(h) = self.header else {
            return;
        };

        let head_node = h as *mut FreePageNode;

        if self.size == 1 {
            self.tail = None;
            self.header = None;
        } else {
            let next = unsafe { &mut *head_node }.next.take();
            if let Some(next_node) = next.as_ref() {
                let next_node = *next_node as *mut FreePageNode;
                unsafe { &mut *next_node }.prev = None;
            }
            self.header = next;
        }

        if self.header.is_none() {
            let _trap = 0;
        }

        self.size -= 1;
    }

    pub fn erase(&mut self, mut nth: usize) {
        if nth >= self.size {
            return;
        }
        if nth == 0 {
            self.pop_head();
            return;
        }

        nth += 1;
        let mut curr_ptr = self.header;
        let mut prev_ptr = 0;
        while nth > 0 {
            match curr_ptr {
                Some(t) => {
                    nth -= 1;
                    let curr_node = t as *const FreePageNode;
                    prev_ptr = t;
                    curr_ptr = unsafe { &*curr_node }.next;
                }
                None => break,
            }
        }

        if nth == 0 && curr_ptr.is_some() && prev_ptr != 0 {
            let prev_node = prev_ptr as *mut FreePageNode;
            let Some(curr_node) = curr_ptr else { return };
            unsafe { *prev_node }.next = unsafe { *(curr_node as *const FreePageNode) }.next;
            self.size -= 1;
        }
    }
}
