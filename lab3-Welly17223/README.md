# Lab 3: Memory Allocator

[作業連結](https://nycu-caslab.github.io/OSC2026/labs/lab3.html)

[Exercise](https://reurl.cc/r0Q0yy)

建議練習過 Exercise 以及 Memory Allocator 的上課簡報之後再來寫這個作業。 Exercise 裡面有包含 Buddy allocator 跟如何保留 Buddy allocator 裡面的 pages 的練習跟解答。

在開始寫 Buddy Allocator 之前，如果有想要做 Startup Allocator 可以先做，因為這是相對簡單的作業，並且在 Buddy 等地方都可以調用這一個 Allocator 來簡化程式碼。

## Advanced Exercise 3 - Startup Allocation

 可以簡單做一個 bump allocator 就可以了，我的實做是：

 1. 將可用的記憶體範圍跟保留的記憶體從 device tree 裡面讀取出來並且排序 
 2. 把 stack top 跟保留記憶體的範圍傳進 startup allocator 。
 3. 當需要一塊記憶體時，就以 stack top 加之前分配過的記憶體先對齊然後往上加，確認沒有跟保留記憶體有重疊，之後就將對應的記憶體位置回傳。

 先做完這個，後面開始做 Buddy 需要一些記憶體時，就可以從這裡分配。

我是後面才發現需要做一個讀取 device tree 裡面 reg 這個 property 的程式，真的太常用了。不然你的程式就會看起來像這樣，又臭又長；或者是去用 parse device tree 的 crate ，助教在 Demo 的時候基本上不會去看之前的程式碼。

```rust
let ptr = ptr as *const u32;
let reg_val = ((unsafe { *ptr.wrapping_add(mem_off) }.swap_bytes() as usize) << 32)
            | (unsafe { *ptr.wrapping_add(mem_off + 1) }.swap_bytes() as usize)
```

## Advanced Exercise 1 - Efficient Page Allocation

最好先想好要怎麼實做 Buddy allocator 以及使用所有記憶體的部分，如此一來才不會寫完之後才開始改程式碼。

## Memory Allocator

建立一個 Memory allocator ，這個 struct 負責管理 buddy system 以及 slab ，外部如果需要記憶體統一由這個 struct 來決定要如何分配，如果是 2 KB 以下就使用 slab ，以上就用 buddy system 。主要需要初始化的是 buddy system ，而 slab 則是在有需要的時候才從 buddy system 要一個 frame 來管理。

[Sysprog 的 Linux Memory 管理講座](https://hackmd.io/@sysprog/linux-memory)

### Buddy System

一個 page 4 KB ，最大 order 至少要是 5 。在上課提供的 Orange PI 上，總共有兩塊記憶體，第一塊是 2 GB ，第二塊是 6 GB ，如果可以至少要將第一塊 2 GB 全部當成 buddy system 管理的記憶體，除了方便做 Advance 2 ，也避免後面的 Lab 會把 buddy system 的記憶體用完。

> 註 available 拼錯了。

page frame 用來記錄這個 page 目前的狀態：
- `Avaliable(usize)`：目前為哪一個 order 的第一個 page ，狀態為可用。
- `BuddyOf`：為某一個 order 範圍裡面的一個 frame。可以用重複把低位的 bit 清 0 來找到目前掌握這個 frame 狀態為 `Avaliable` 的 frame 。
- `Occupied(usize)`：與 Available 一樣，不過代表這個 frame 被分配出去了。
- `OccupiedSlab`：代表目前是被 slab 占據。

```rust
pub struct PageFrame {
    state: PageState,
    base_addr: usize,
}

#[derive(Debug, Clone, Copy)]
pub enum PageState {
    Avaliable(usize),
    BuddyOf,
    Occupied(usize),
    OccupiedSlab,
}
```

在初始化階段，給每一個 memory block 建立一個 memory allocator 。在此先計算需要多少的 page frame ：
```math
\text{N}_\text{page frame} = \frac{\text{size}_\text{memory\_block}}{\text{size}_\text{page}}
```
然後初始化 page frame：
```rust
for i in pages.iter_mut() {
    *i = PageFrame {
        base_addr: range.base + memory_page_offset,
        state: PageState::BuddyOf,
    };
    memory_page_offset += PAGE_SIZE;
}
```
由於要建立 free list 將每一個 free page 串起來，在沒有 allocator 的情況下，也要先給每一個 frame 建立一個 link list 的區域。這裡採用雙向 link list ，當要合併 page 的時候需要將 free list 中段的 page 取出，透過雙向 link list 可以做到 $O(1)$ 的存取速度。

程式的行為基本上 exercise 一致，不過在操作 link list 時要注意有沒有邊緣情況要處理。

這裡 link list 裡面的指標雖然都是由 `usize` 表示，但是後來想想應該直接用 `*mut T` 來代表比較好，雖然這樣需要手動實做 `Send` 跟 `Sync` ，不過會讓程式碼的可讀性更好。

> 註：這裡的 buddy allocator 有一些 bug 是在後面的 Lab 發現的。
> - 在 free list 嘗試 pop 中段時沒有判斷是否為尾部，因此會出錯。在 Lab 6 的程式裡面有做修復。
> - 有些指標寫入操作使用 `unsafe { *ptr } = value` 這樣的形式，結合 [lab 2 筆記的補充說明](/lab2-Welly17223/README.md#補記-rust-指標操作與全域變數)是無法做寫入的，應該改成 `unsafe { &mut *ptr } = value` 。也是在 Lab 6 修復。

補充找出 pages 的 pair 的算式：
```math
\text{index}_{pair} = \text{index}_{current} \oplus  (2 ^ {order} )
```

### Dynamic Memory Allocator

參考網路上 Linux slab allocator 所寫出來的簡化版本，不會在 slab 為空時自動回收。（[知乎連結](https://zhuanlan.zhihu.com/p/490588193)）

在分配記憶體時當容量小於等 1024 就會透過 slab 來分配，分配的數量會 round up 到最近的 2 的次方。在透過 page allocator 獲得一個 page 作為 slab frame 用，在這 4KB 裡面，會在最前面記錄這個 slab 的相關資訊；同時此 slab frame 也是環狀雙向 link list 的其中一部分，當這個 slab frame 滿了之後，會建立新的 slab frame ，並讓現在這個 slab frame 連接到他。

之所以要把 16 bytes 的 Header 獨立出來是因為 16 bytes 的 Slab frame 在扣掉 header 之後，還是會有 250 個 entry ，因此需要 250 個 bits 用來追蹤這些 entry 的使用狀況。而在 entry size 為 32 到 1024 的 slab frame 則是用 128 個 bits 就夠了。

在讀取 frame header 時需要確認他的 type 是 32 \~ 2048 bytes 或是 16 bytes 的 slab ，因此透過類似 C 裡面的 `sockaddr` 、 `sockaddr_in` 以及 `sockaddr_in6` 類似的存取方式，透過記憶體 Layout 一樣，所以存取到的 `class` 這個 entry 都是在第 0 個 offset ，進而用來分辨是哪一種形態的 allocator ，並且做 `alloc` 、 `free` 等。
```rust
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
```

如果要做回收及增加效率應該可以透過在 slab list 裡面建立三個 list 分別儲存 狀態為 full 、 partial free 以及 free 的 slab ，然後當 free slab 太多時做回收。

### Advanced Exercise 2 - Reserved Memory

照著 exercise 做應該就沒有問題了。

## Rust 的 Global Allocator

可以將我們寫好的 memory allocator 實做 `core::alloc::GlobalAlloc` 這個 trait ，之後就可以使用 `alloc` 這個 crate 裡面的功能了，包含之前提到的 `Vec`、`BTreeMap`、`BinaryHeap`、`String` 等資料結構以及 `Arc`、`Box` 等 smart pointer 。

在實做的時候有些要注意的地方：
1. 由於 Rust 對於 `global_allocator` 要求必須是 `static` 且不可變，因此需要讓我們自己假設的 Global Allocator 擁有內部可變性，在這裡一樣使用 `Mutex` ，不過要注意這會導致 deadlock ，如果你在 allocator 內部呼叫有可能會使用到 dynamic memory 的函數或是 interrupt 。
2. Rust 會需要 layout 這個參數，不過在 Kernel 裡面，我們先前實做的 memory allocator 會 aligned 到 allocate memory 的大小，因此要注意不要用 dynamic 記憶體分配超過本身大小 aligned 的變數。

## C 的 `kmalloc`
可以先做好給 kernel 用的關於記憶體的 API 像是 `kmalloc` 、…… 供後續使用。

## 心得

第一份需要花很多時間上去的作業，建議多留時間來寫，大約需要 20 小時。雖然目前沒有 memory allocator ，因此整個實體記憶體的所有區域都可以使用，不過要如何<ruby>有效率<rt>低時間複雜度</rt></ruby>的方式管理以及要如何設定 reserved memory 這些概念雖然想起來、看起來不難，但是要如何兼顧 Rust 的<ruby>語法<rt>哲學</rt></ruby>和實做細節的複雜度確實讓我在寫這一份作業時花的許多時間設計。
