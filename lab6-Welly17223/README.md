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

這裡設計有參考 [Redox OS Kernel] ，一個由 Rust 寫成的作業系統，主要是將 [virtual memory](#補充rubyvirtual-memory-areart-v-m-a-rtruby) 以及 physical address 以及 virtual memory 分成兩個 struct ，提供更清晰的語義，並且可以提供各自的轉換 method 或是 virtual memory 需要計算的 virtual page number 等，不過比較麻煩的是需要額外實做加減法等 trait 才能直接參予 `usize` 等整數運算。Page table 以及 Page table entry 額外宣告也是一樣的道理。

> 註：為了讓開機的程式可以在 `0xfff_ffc0_0020_0000` 的地方，因此我將 bootloader 改成會 self relocation ，並且將 UART 讀進來的程式寫到 physical address `0x20_0000` 的地方。

[Redox OS Kernel]: https://gitlab.redox-os.org/redox-os/kernel

```rust
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

宣告一個全域變數用來裝 kernel space 的 root page table 。最後記得在寫入 `satp` 之後，需要用 `sfence.vma` 清除 transition look aside buffer 。

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

### 補充：<ruby>virtual memory area<rt> v m a </rt></ruby>

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

簡單來說 `vma` 負責的是管理從 `0x0` 到 `0x0000_003f_ffff_ffff` 的使用者記憶體空間，標示了哪些記憶體被分配出去，哪些記憶體可用，下圖是 Linux kernel 實做 `vma` 的概念圖。如果沒有要做 advanced exercise ，那麼可以跳過 `vma` ，做起來應該會相對輕鬆許多。不過後面的筆記還是會以實做 `vma` 的前提來介紹。

![Virtual memory area](https://encrypted-tbn0.gstatic.com/images?q=tbn:ANd9GcQsnUdmr7b1-WcQqLDCEXMO27MON59ZBtYDdqVkb1uvJx2AUY5j7Wnzr2g&s=10)

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
```
將此結構加入到 thread control block 裡面，並且用 Option 包住，如果是 kernel thread 就不需要這一個結構。

裡面的 `vm_area` 儲存了已經標注這個 process 可以使用的記憶體空間，當有 page fault 的 interrupt 時程式會優先查看錯誤存取的記憶體空間是否有記錄在 mapping 裡面，如果沒有，就當作違法的存取，直接 segmentation fault 退出，如果有那麼就確認有沒有實體映射以及權限是否正確。而 `vm_free_*` 則代表記憶體中間的洞，當需要映射實體記憶體時，就從空洞裡面尋找可用的記憶體。使用這一個結構，可以同時管理 stack 、 text 以及後面 advanced exercise 所要求要分配的記憶體。

### Revisit System calls

### Context Switch and Video player

## `mmap`

## Page Fault Handler & Demand Paging

## Copy on Write

## 心得
