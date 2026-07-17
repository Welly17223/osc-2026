# Lab 6 Virtual Memory

上課講義：[virtual memory](https://people.cs.nycu.edu.tw/~ttyeh/course/2026_Spring/IOC5226/slide/page-I.pdf)、[paging](https://people.cs.nycu.edu.tw/~ttyeh/course/2026_Spring/IOC5226/slide/page-II.pdf)、[RISC-V paging](https://people.cs.nycu.edu.tw/~ttyeh/course/2026_Spring/IOC5226/slide/page-III.pdf)

[作業連結](https://nycu-caslab.github.io/OSC2026/labs/lab6.html)

[Exercise](https://reurl.cc/O6Qzay)

目標是設定 RISC-V 的 SV39 虛擬記憶體，並且不用 `ASID`。作業大魔王終於來了！這一篇難點在於 virtual memory 相關的程式不太好除錯，有時候設定錯誤程式會直接卡死，以及 virtual memory 整個操作以及概念覆蓋範圍太廣，課程講義無法覆蓋全部，有很多應用以及實做方面的細節要自己上網查詢。

這一次作業使用 SV39 的 virtual memory 設定，最多可以有三層的 page table ，硬體會用是否有除了 valid bit 以外的 bit 為 1 來判斷這一個 entry 是一個 entry 或是指向另一個 page table 。

虛擬記憶體是作業系統管理 user process 裡面很重要的一環，通過操作 page table ，可以達成 user process 存取記憶體的權限控制、demand paging 、fork 的 COW 以及將 virtual memory 映射到硬碟或是其他 I/O 設備上；更有甚者，可以通過映射相同實體記憶體給不同的程式以達成分享記憶體的效果。

## QEMU virtual memory 除錯

可以在 QEMU 的執行參數加上 `-monitor telnet:127.0.0.1:45454,server,nowait` 來開啟 [QEMU monitor] ，並且用 `telnet 127.0.0.1 45454` 連線。在 QEMU monitor 裡面可以查看目前虛擬機的資訊，其中也包含 virtual memory 的分配情況，使用 `info mem` 即可查看。以下是我完成 kernel space mapping 之後可以看到的輸出：
```
(qemu) info mem
vaddr            paddr            size             attr
---------------- ---------------- ---------------- -------
ffffffc000000000 0000000080000000 0000000000050000 r---gad
ffffffc000050000 0000000080050000 00000000001b0000 rwx-gad
ffffffc000200000 0000000080200000 000000003fe00000 rwx-gad
ffffffc040000000 00000000c0000000 00000001c0000000 rwx-gad
ffffffc280000000 000000000c000000 0000000000600000 rw--gad
ffffffc280600000 0000000010000000 0000000000200000 rw--gad
```

[QEMU monitor]:  https://qemu-project.gitlab.io/qemu/system/monitor.html

## Virtual Memory in Kernel Space

目標是讓程式能夠在虛擬地址上面跑。在 RISC-V 上面，以 `0xffff_ffc0_0000_0000` 為 kernel space 的起點，將整個 physical memory 的地址以此向上做 linear mapping 。同時還要改
- linker script 將 base address 改成 `0xffff_ffc0_0000_0000`
- memory loader 將內部的 linked list 改成指向虛擬地址、Global Allocator 回傳虛擬地址。

![](https://nycu-caslab.github.io/OSC2026/_images/Riscv_SV39_Memory_Layout.png)

![](https://nycu-caslab.github.io/OSC2026/_images/lab6_sv39.png)

這裡設計有參考 [Redox OS Kernel] ，一個由 Rust 寫成的作業系統，主要是 [virtual memory area](#補充virtual-memory-area-vma) 以及將 virtual memory 和 physical address 以及 virtual memory 分成兩個 struct ，提供更清晰的語義，並且可以提供各自的轉換 method 或是 virtual memory 需要計算的 virtual page number 等，不過比較麻煩的是需要額外實做加減法等 trait 才能直接參予 `usize` 等整數運算。Page table 以及 Page table entry 額外宣告也是一樣的道理。

> 註：為了讓開機的程式可以在 `0xfff_ffc0_0020_0000` 的地方，因此我將 bootloader 改成會 self relocation ，並且將 UART 讀進來的程式寫到 physical address `0x20_0000` 的地方。

[Redox OS Kernel]: https://gitlab.redox-os.org/redox-os/kernel

```rust
// oslib/src/virtual_mem/mod.rs

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct PhysicalAddress(pub usize);

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct VirtualAddress(pub usize);

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct PageTableEntry(pub usize);

#[derive(Debug)]
#[repr(C, align(4096))]
pub struct PageTable {
    pub entries: [PageTableEntry; ENTRIES_PER_TABLE],
}
```

### `PageTable` 以及 `PageTableEntry` 的實做細節

如何轉換 virtual memory 以及 physical memory 呢？如果像是 QEMU 的 begin address 是從 `0x8000_0000` ，而 Orange PI 從 0 開始。此時定義一個全域變數，並且在 memory allocator 抓取全局記憶體 layout 時，將其設定為開始位置為排序過後開始位置最小的記憶體，並且在後面使用：

```rust
#[inline]
pub fn virt_to_phy(va: VirtualAddress) -> PhysicalAddress {
    PhysicalAddress(va.addr() - PAGE_OFFSET.addr() + phy_begin())
}
```

在 `PagetableEntry` 裡，儲存的 `PPN` 有可能是一個 physical address 指向映射的實體記憶體，也有可能是指向一個 page table ，為了方便確認 entry 裡面儲存的 page 是哪一種，就有了 `to_leaf_ref` 。

```rust
impl PageTableEntry
    #[inline]
    pub fn is_leaf(&self) -> bool {
        self.0 & PROP_MASK == PTE_V
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
```

`try_new_entry` 是在需要為大體積映射的 entry 改成由一個 page table 描述時使用，設計上只能用在 **kernel linear mapping 的區域**、**沒有 mapping 的記憶體位置**或是 **entry 是 page table 的地方**，否則有可能會有 memory leak 。當這個映射是 1 GB 的映射，但是我們需要修改以 2 MB / 4 KB 為單位的記憶體權限時，就可以使用這個函數將大範為映射的 entry 改成指向下一層 level 的 page table 。主要用在設定 reserved memory 的地方以及建立新的 4 KB level 的 page table。

`set_prop_range` 設計上使用泛型 `SliceIndex<[PageTableEntry], Output = [PageTableEntry]>` 讓傳入的值可以是任意範圍描述如 `0..256` 或是 `0..=255` 等。

至於 Clone trait 以及 Drop 等記憶體安全的措施與 User process 有關，將在後面介紹。

```rust
impl PageTable {
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

}
```

### Supervisor Address Transition and Protection (`SATP`) Register

一樣先上 `satp` bit field ，這裡嚴格要求 `PPN` 需要 4096 bytes aligned ，我們之前寫的 buddy allocator 本來就會分配 4 KB aligned 的 page 因此不需要額外改動：

|Bit|Name|
|-|-|
|0-43|`PPN`|
|44-59|`ASID`|
|60-63|Mode|

以及 `make_satp`：

```rust
#[inline]
pub fn make_satp(pa: PhysicalAddress) -> usize {
    (pa.0 >> 12) | SATP_SV39
}
```

或是用 `riscv` 提供的函數：
```rust
use riscv::register::satp;
satp::set(satp::Mode::Sv39, 0, (&raw const PGD as usize) >> 12);
```

宣告一個全域變數用來裝 kernel space 的 root page table 。最後記得在寫入 `satp` 之後，需要用 `sfence.vma` 清除 transition look aside buffer 。使用 `sfence.vma {addr}, {asid}` 可以減少 transition look aside buffer 清除的範圍，增加效能，也可以使用 `riscv` crate 包裝的 `riscv::asm::sfence_vma` 以及 `riscv::asm::sfence_vma_all` 來減少手寫 assembly 導致的錯誤。

### Identity Mapping

這裡只做最簡單的：將 kernel 程式所佔的記憶體做 Mapping ，除此之外，由於程式開始之後就要讀取 device tree ，因此也順做了 mapping 。至於將整個 memory space 都映射到 root page table 則是在初始化 memory allocator 的時候。在跳到 virtual memory 之後，必須 drop 掉原本的 identity mapping 以免影響之後 user space 的設定。由於追求趕緊跳到 virtual memory ，因此在做 mapping 時只修改 root page table （每次映射 1 GB 的記憶體）。

對應的啟動的 assembly 也要做修改，要負責呼叫初始化 virtual memory 的函數、跳到高位 memory 以及呼叫 drop identity 的函數，由於使用 `la` 指令會根據目前的 program counter 回傳 symbol 的相對位置，因此可以利用這個特性獲取在低位以及高位的各個 symbol 的記憶體位置。我在啟動程式 `start.S` 裡面做了滿詳細的註解，可以過去查看。 

### Map the Kernel Space

在 memory allocator 初始化完可用 memory 區塊之後，根據記憶體的大小以及區塊將整個 physical memory 映射到 virtual memory 。記得將 dynamic allocator 以及 buddy allocator 裡面的 linked list 所指向的地址改成 virtual memory 。順帶一提，像是 UART 等設備的記憶體位置並不在 physical memory 的範圍裡面，因此需要額外使用 `io_remap` 函數將設備的實體記憶體映射到 page table 上。

### Finer Granularity Paging

在初始化完 memory allocator 之後，終於可以分配新的 page table 以及設定 reserved memory 的屬性了。為了最大化利用記憶體（這個沒有特別要求就是了），因此目標是儘量使用 level 較高的 page table 完成映射，只有在目標 mapping 的大小不夠時才使用 level 較低的 page table 。演算法如下：

1. 對於小於 2 MB 的保留記憶體，使用一個 level 0 的 page table 完成覆蓋。
2. 對於大於等於 2 MB 的保留記憶體，先用 2 MB 的 page table entry 完成覆蓋直到 1 GB aligned ，之後用 1 GB 的 page table entry 覆蓋範圍，最後再用 2 MB 的 page table entry 做最後的覆蓋收尾。

## Virtual Memory in User Space

設定 user process 的記憶體，主要設定 text 以及 stack 就好，目前沒有 ELF parser 因此無法分別哪些資料是 `text` / `rodata` / `bss` 。

### 補充：virtual memory area (`vma`)

要做 `MMAP` 、 demand paging 或是只是想要管理 user 記憶體都建議實做一下，然而課程上並沒有提及，可能教授認為我們都會吧？總之以下是我主要參考的資料：
- [Linux Kernel - Virtual Memory management](https://hackmd.io/@naup96321/rkJwaqfIkx)
- [Redox OS Kernel]： 主要是 `src/context/memory.rs` 有關於 virtual memory area 的實做程式碼，包含下面兩個資料結構：
```rust
#[derive(Debug)]
pub struct UserGrants {
    // Using a BTreeMap for its range method.
    inner: BTreeMap<Page, GrantInfo>,
    // Holes ordered by memory address for merging adjacent holes
    holes_by_addr: BTreeMap<VirtualAddress, usize>,
    // Holes ordered by size then start address for fast allocations
    holes_by_size: BTreeSet<(usize, VirtualAddress)>,
}

#[derive(Debug)]
pub struct GrantInfo {
    page_count: usize,
    flags: PageFlags<RmmA>,
    // TODO: Rename to unmapped?
    mapped: bool,
    pub(crate) provider: Provider,
}
```
- [以前交大的 Online course](https://youtu.be/1zMipcUhsOs?si=-R6oG6fuBZZLhJbG)：這個講的很詳細，值得一看。

簡單來說<ruby> virtual <rt>v</rt> memory <rt>m</rt> area <rt>a<rt></ruby> 負責的是管理從 `0x0` 到 `0x0000_003f_ffff_ffff` 的使用者記憶體空間，標示了哪些虛擬記憶體地址有映射了以及映射到哪裡，哪些虛擬記憶體位址可用，下圖是 Linux kernel 實做 `vma` 的概念圖。如果沒有要做 advanced exercise ，那麼可以跳過 `vma` ，做起來應該會相對輕鬆許多。不過後面的筆記還是會以實做 `vma` 的前提來介紹。

![Virtual memory area](https://encrypted-tbn0.gstatic.com/images?q=tbn:ANd9GcQsnUdmr7b1-WcQqLDCEXMO27MON59ZBtYDdqVkb1uvJx2AUY5j7Wnzr2g&s=10)

### 補充：`PageTable` 如何 Clone 以及 Drop

由 `vma` 的管理者管理 `pgd` 的 clone 以及 drop。 在 fork 時會觸發 `vma` 的 clone ，此時 root page table 的 kernel space 部分保持不變，至於 user space 則是如果這個 entry 是指向一個 page table 則遞迴的分配一個新的 page table ，並且複製裡面的 entry 過去，如果這個 entry 指向一個 physical page frame ，那麼則將其 reference count 加一：Drop 的實做方法與 Clone 相似。 

```rust
// oslib/src/virtual_mem/vm_area.rs

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
```

### PGD Allocation

首先是建立一個用來管理 user space virtual memory 空間的結構，同時也負責管理 root page table 裡面的 entry ：
```rust
#[derive(Default, Debug, Clone)]
pub struct Manager {
    vm_area: BTreeMap<VirtualAddress, AreaEntry>,
    vm_free_addr: BTreeMap<VirtualAddress, usize>,
    vm_free_size: BTreeSet<(usize, VirtualAddress)>,
    pub pgd: Box<PageTable>,
}

#[derive(Default, Debug, Clone)]
struct AreaEntry {
    size: usize,
    flags: usize,
    backed: Provider,
}

#[derive(Debug, Clone)]
pub enum Provider {
    Anonymous,
    File(Box<[u8]>),
}
```
將此結構加入到 thread control block 裡面，並且用 `Option` 包住，如果是 kernel thread 就不需要這一個結構。

裡面的 `vm_area` 儲存了已經標注這個 process 可以使用的記憶體空間，當有 page fault 的 interrupt 時程式會優先查看錯誤存取的記憶體空間是否有記錄在 mapping 裡面，如果沒有，就當作違法的存取，直接 segmentation fault 退出，如果有那麼就確認有沒有實體映射以及權限是否正確。而 `vm_free_*` 則代表虛擬記憶體位址中沒有被分配映射的部分，當需要映射實體記憶體時，就從空洞裡面尋找可用的記憶體。使用這一個結構，可以同時管理 stack 、 text 以及後面 advanced exercise 所要求要分配的記憶體，是一個寫的時候有點累，但是可以很高效管理虛擬記憶體空間的結構。

已經設定好 `vma` 之後，就可以分配記憶體並且寫入 root page table 了。做法也很簡單，如果是不實做 advanced exercise 的話先判斷 `Provider` ，並且用 Exercise 6-2 裡面的 `pagewalk` 將分配的實體記憶體位址寫入 root page table 裡面。為了解決 external fragmentation ，給使用者做 mapping 的記憶體都使用 4 KB 大小的 page 。

### Revisit System calls

#### `sstatus.SUM`

在 supervisor mode 要存取有標記 user bit 的記憶體位址時需要將 `sstatus.SUM` 這一個 bit 設定為 1 。在修改如 `UartRead` 以及 `UartWrite` 的 system call 時有兩種方法：
- 搜尋 page table 將使用者給的 virtual address 轉換成 kernel linear mode mapping 的 virtual address 並且根據此 address 修改。
- 根據使用者給與的資料與長度建立同樣長度的記憶體，在 system call 結束時，再將結果複製過去。

#### `Exec`

清空重新建立一個 `vma` ，把舊的 drop 掉，並且建立新的 text 以及 stack 映射。

#### `fork`

如果不實做 copy on write 的話會長成這樣：遞迴式的複製 page table 裡面 page entry 的內容，並且建立新的 page table 將原本 page entry 的內容寫進去，最後記得要把 `Context` 裡面 `satp` 的 physical address 改成新的 root page table 的實體地址。如果 copy on write 的話就把 page entry 的內容改成 read only 就好了。

#### Signal 系列的 system call

助教在 Demo 時好像不會看這一方面的內容。如果直接將 kernel space 的 函數直接設定為 signal 的 return 函數，會出現 user mode 無法讀取與執行沒有 "U" bit 的記憶體，因此需要建立這一個函數在 user space 的記憶體映射；同理， user mode 的 return 函數預設是做 exit 的 system call ， 因此也需要做類似的處理。在做映射的時候，同時最好也考慮到不要讓 user mode 的程式執行非預期的程式碼，因此也要避免將多餘的程式映射進 kernel mode 。

為了減少實做的複雜度，我們這裡會將 user mode 做 system call 的兩個函數編譯在同一個區塊 `.text.user` ，並且要讓這些程式編譯做 4096 bytes 對齊，方便計算 offset 。因此要做相對應的 link 設定：
```rust
// oslic/src/thread.rs

#[unsafe(naked)]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.user")] // new
/// # Safety
/// only use when return from a userspace signal handler
pub unsafe extern "C" fn sig_ret() {
    naked_asm!(
        r#"
        li a7, 11
        ecall
        "#
    )
}

/// This function invoke exit system call for u mode thread
#[unsafe(naked)]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.user")] // new
pub extern "C" fn u_mode_do_exit(code: isize) -> ! {
    naked_asm!(
        r#"
        li a7, 6
        ecall
        "#
    )
}
```

在 linker script 裡面使用 symbol 來記錄 user text 的範圍：
```ld
/* link_script.ld */

SECTIONS {
  /* ... */
  .text : {
      /* ... */
      . = ALIGN(4096);
      __user_text_start = .;
      *(.text.user)
      __user_text_end = .;
  } > program
  /* ... */
}
```

最後，在建立 user process 時，將這一部分的 text 複製到一個大小為 4096 的 struct ，並且將其餘部分設定為 0。

```rust
// osclib/src/thread.rs

unsafe extern "C" {
    pub static __user_text_start: usize;
    pub static __user_text_end: usize;
}
// ...
impl ThreadControlTable {
    pub fn new_user_thread(...) -> Self {
        // ...      
        let user_text = unsafe {
            &*ptr::slice_from_raw_parts(
                &__user_text_start as *const usize as *const u8,
                (&__user_text_end) as *const usize as usize
                    - (&__user_text_start) as *const usize as usize,
            )
        };
        // ...
        let mut user_text_copied: Box<[u8]> = Box::from([0u8; 4096]);
        user_text_copied[..user_text.len()].copy_from_slice(user_text);

        let user_text_base = vm_mapper
            .map_file(user_text_copied, virtual_mem::PROT_USER_TEXT)
            .unwrap();

        // ...
    }
}
```

至於 signal stack 的部分就交給<ruby> `vma` 的 `map` API <rt>無敵的白金之星</rt></ruby>處理就好了。  

### Context Switch and Video player

把前面的內容實做出來之後，要完成這兩個應該不難，因此就不過多贅述了。

## `mmap`

把前面 `vma` 做好的 mapping API 拿來用就可以了。

## Page Fault Handler & Demand Paging

在 Exception 裡面，只有名稱裡面有 "Page Fault" 的才是存取 virtual memory 發生的 exception ，也就是 Instruction Page Fault、load page fault 和 store AMO page fault 。在沒有寫 fork 的 copy on write 的情況下，需要做的就是：
1. 判斷存取的記憶體是否有在 `vm_area` 裡面，如果有則確認是否有做實體映射。如果不在 `vm_area` 範圍裡面就將此 process 結束。
2. 如果成功完成實體映射，就先 return 。
3. 如果實體映射已經存在卻仍然發生錯誤，那麼就是存取權限發生錯誤，可以直接結束 process 。

## Copy on Write

在 fork 時將所有有分配記憶體的 page table entry 改成 read only 讓他們在做儲存時觸發 page fault ，並且在 page fault 時判斷這個已經分配記憶體的 page table entry 的 property 是否與 `vm_area` 裡面記錄的一致，如果缺少了 write 的 bit 就複製舊的 page table entry 到新的 page 裡面，並且 drop 掉舊的 entry 。除此之外，在複製 page table entry 時，需要將其在 page frame 裡面的 reference 數量加一，讓我們在釋放記憶體時要等 reference 數量歸 0 才歸還 page 。

## 心得

如同前面說的，這一份作業的難點之一就是十分難以除錯，因為當 `satp` 或是 page table 設定錯誤時並不會報錯，如果可以在前期設定一個前期的 interrupt handler 將 page fault 的錯誤值印出來肯定可以降低除錯難度——不過我是在寫完作業之後才知道的，寫作業時只在 debugger 裡面瘋狂設斷點，真的是很苦。另外就是本人見識淺薄能力不足，在做作業時光是搞定 virtual memory 就拼盡了全力，沒有時間再去搞 `vma` 因此在實做 advanced exercise 時繞了很多彎路！看了前面的作業講解應該不難發現，這三個 advanced exercise 基本上就是 `vma`、`mmap` 的延伸應用，只要將 `vma` 相關的 API 做好，後面就是將 API 組合一下就可以了。不過在實做 `vma` 的路上，也多虧了 Rust 提供的 B Tree ，基本上就是不需要多花時間在對資料結構除錯，不敢想象如果用 C 的話會是怎麼樣的一番光景。
