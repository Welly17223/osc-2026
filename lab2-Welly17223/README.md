# Lab 2: Booting

[作業連結](https://nycu-caslab.github.io/OSC2026/labs/lab2.html)

[Exercise](https://reurl.cc/6b1lLO)

在開始寫 Lab 2 之前，建議先閱讀上課投影片 Bootloader 跟 Device Tree 了解相關概念。

## UART Bootloader

在 Lab 1 時，每次上板子測試都需要重新將程式傳到 SD 卡上面，因此這裡要寫一個能夠從 UART 上接受程式碼的程式，在開機的時候讓我們將 Kernel 的程式碼傳到上面。

整個程式要做的事情包含：
1. 接收 Kernel
2. 跳過去
3. 寫一個透過 UART 將 Kernel 穿過去的程式（助教推薦使用 Python 寫，不過我用 C）

整體而言應該沒有甚麼困難的地方，就是 read and write 而已。這裡有兩個小坑，一是由於宿主機傳送的 UART 有可能過快導致掉包，因此需要一次傳一點，等到確定傳過去在開始傳下一個（助教好像沒有遇到的樣子）；另一個是不能直接調用前面寫好的 UART 的 `getc` ，因為他會將 `'\r'` 轉換成 `'\n'`，這裡需要一個不會轉換的讀取方法。另外是在跳過去之前，由於有對「儲存 text 」的記憶體做修改因此需要使用 `fence.i` 指令清空 i-cache。 

完成這個程式之後，就可以給 bootloader 制作 Image 並且放入 SD 卡裡面。之後應該除了要換 `initrd` 之外，就都不用重新插拔 SD 卡了。Makefile 的改動：加上了 bootloader 的建立方法，並且將 Image 裡面包的內容改成 bootloader 。

> By the way ，我後來發現跳到 kernel 應該不用另外寫組語，在 Rust 裡面就可以執行了，所以可以忽略 `start.s` 裡面的部分內容。

### (Advance) Bootloader Self-Relocation

這裡要做的事情包含：
1. 編譯出位置無關（position-independent）的程式碼，可以參考 [Pre-MMU execution](https://docs.kernel.org/arch/riscv/boot.html#pre-mmu-execution) 來編譯。
2. 找一塊硬體空間將自己移過去（可以 hardcode 或是在 Device Tree 尋找）
3. 把自己移過去並且 `fence.i`

簡單解釋一下 position-independent kernel 兩個參數的含義：
- `-fno-pie`: 不使用 global offset table 來定位全域變數的位置。
- `-mcmodel=medany`: 強制產生 PC 相對地址，所有 label 都回在 pc $\pm$ 2GB 之間。
對應到 Rust (LLVM) 會是這兩個 Flags： `relocation-model=static`、`code-model=medium`。詳細可以參考 [Codegen Option](https://doc.rust-lang.org/rustc/codegen-options/index.html#code-model)。

我對於 Self relocation 的做法基本上就是在進入清楚 `bss` 之前寫一段組合語言將程式複製過去，如果有需要，也可以加上搜尋 Device Tree 的程式碼尋找可用的記憶體。這個 Advance 算是滿值得做的，因為在 Lab 6 會用到，現在先做好後面沒煩惱。

> 註：由於我再 Lab 6 裡面由於程式需要才完成了 Self relocation 的相關程式，因此 Lab 3 ~ Lab 5 的程式裡面仍使用的是沒有 Self relocation 的 bootloader 跟 Linker file 在參考時請注意。Lab 2 我應該已經改成 Self relocation 的 bootloader 了。

## Device tree

Parse Device Tree 的部分應該算是頗簡單的，跟著 Spec、Exercise 刻就可以了。當初也是參考網路上很多關於 raw pointer 的教學。不過現代語言的強大抽象能力也是 Rust 的優點之一，可以透過把遍歷 Device Tree 的過程抽象成一個 Rust 的 iterator 讓之後如果需要新的函數也需要遍歷 Device Tree 的時候就不用重寫這一部分的程式了；這裡做一個比較，在做 iterator 前，由於很多功能都需要遍歷 Device tree ，因此有很多重複的程式碼，整個檔案高達 600 多行，在實做 Iterator 之後就只剩 300 多行了。就算是使用 C 來寫也可以考慮類似的做法，讓程式更加簡潔。實做方法也可以參考網路上的 [crate](https://docs.rs/fdt/latest/fdt/index.html) 。

記得要用 `[repr(C)]` 標注 struct 才能在執行時用 C 語言的標準來存取對應 offset 的變數。格式裡面定義的 `uint32_t` 可以直接對應到 Rust 裡面的 `u32`，記得在存取資料時要把資料用 
`swap_bytes` 轉成 little-endian 。
```rust
#[repr(C)]
struct FdtHeader {
    magic: u32,
    totalsize: u32,
    off_dt_struct: u32,
    off_dt_strings: u32,
    off_mem_rsvmap: u32,
    version: u32,
    last_comp_version: u32,
    boot_cpuid_phys: u32,
    size_dt_strings: u32,
    size_dt_struct: u32,
}

#[repr(C)]
pub struct FdtProp {
    len: u32,
    name_off: u32,
}
```

> 註：Device Tree 的 `path_all_offset` 這一函數我後來發現有 Bug ，有在 Lab 3 做修改，要參考請移駕到 Lab 3 的同名檔案裡。

> 註：Device Tree 裡面每一個 property 的 parse 方式都不太一樣，所以最好是把 property 的 pointer 讀取出來之後再各自 parse 。否則這麼多 property type 甚至有些是特殊的要一個一個支援應該是十分痛苦~，不過像是 `reg` 這個常用的 property 倒是可以優先支援就是了。 

向上 aligned 的程式碼，很多地方都用得上：
```rust 
#[inline]
fn align(n: u64, byte: u32) -> u64 {
    let mask = (byte - 1) as u64;
    (n + mask) & !mask
}
```

接下來是在 Device Tree 裡面尋找 UART 的地址。與 Lab 1 不同，我做了一些改動給 UART 建立一個 struct ，並且實做了 `core::fmt::Write` 這個 trait 。直到 Lab 4 我才將 UART 改成完全體：在 QEMU 跟 Orange PI 上的 UART 模組裡面的 Compatible (格式為 `"manufacturer,model"` 更多請詳閱 Device Tree 的規格書) 可以看到一個是 `ns16550` 另一個是 `pxa-uart` ，這兩者的 Driver 在 Linux Kernel 裡面是在 `/drivers/tty/serial/8250/` 這個資料夾下面。以結論而言，雖然 `pxa-uart` 有整合 DMA 、省電等設計，但是如果只是想要達成讀取這學期作業會用到的 MMIO Register ，並且是在 Open SBI 已經初始化過的 S Mode 之上時，是可以以相同的邏輯來操作的。在操作時，比較重要的是 device tree 裡面的 `reg` 跟 `reg-shift` 這兩個參數，分別代表 base address 跟 base address 與下一個 MMIO Register 的 offset ，各個不同 register 的 address 為 `base address + (register_offset << reg-shift)` 。總之， QEMU 跟 Orange PI 上面的 UART 可以用同一種 driver 操作，只要設定好 base address 跟 register shift 就好了。

其實這裡就可以把 UART 設成全域變數了，然後用 `Once` 或是 `Option<T>` 來延遲初始化的時間，個人認為用 Once 在語義上比較直覺也比較符合 Rust 的慣例。我是到了後面的 Lab 才這麼做。

## Initial Ram disk
同上，一樣記得 `[repr(C)]`、將遍歷抽象成 Iterator ，然後 Exercise 裡面有相關說明可以參考。在有 User program 出現之後，基本上就是從這個地方載入程式的。其實這裡有一點是可以說的，也就是如果有一個檔案的檔名叫做 `TRAILER!!!` 那麼這個 Iterator 的 `next` 會直接回傳 `None` ，或許要加入其他判斷讓整個遍歷更加 robust 一點會比較好。

```rust
#[repr(C)]
pub struct Cpio {
    pub magic: [c_char; 6],
    pub ino: [c_char; 8],
    pub mode: [c_char; 8],
    pub uid: [c_char; 8],
    pub gid: [c_char; 8],
    pub nlink: [c_char; 8],
    pub mtime: [c_char; 8],
    pub filesize: [c_char; 8],
    pub devmajor: [c_char; 8],
    pub devminor: [c_char; 8],
    pub rdevmajor: [c_char; 8],
    pub rdevminor: [c_char; 8],
    pub namesize: [c_char; 8],
    pub check: [c_char; 8],
}
```

## 檔案架構改變
一開始為了讓一些 library 性質的程式可以被 bootloader 以及 main 複用，因此將 library 獨立出來，變成一個獨立的 crate 。不過在實做 memory allocator 之後，由於 bootloader 是不會做 memory 相關的事情，因此會導致修改之後的部分 library 程式碼無法使用，因此我最後是把 bootloader 所需要的 minium 程式碼獨立出來放在 bootloader 裡面。

# 補記 —— Rust 指標操作與全域變數

Rust 雖然一向以記憶體安全出名，但是仍然保有了 pointer ，可以安全的加減，並且以 `unsafe` 的方式來讀取、寫入。寫過 C 應該都對於指標的操作相當熟悉，而 Rust 裡面的指標操作大致上與 C 相同：
- 可以從整數形態用 `as` 直接轉變
- 可以從用 `&raw const` 、 `&raw mut` 從變數轉變，也可以從 reference 直接轉變
- 可以從一種指標轉換成另外一種
- `unsafe { &*ptr }` 可以將指標轉換成 reference 、 `core::ptr::slice_from_raw_part(ptr, len)` 則可以將指標 `*const T` 轉換成 `*const [T]`。
- 解引用（deference）取得指標原本所持有的變數的所有權，因此要小心使用。在 `#[derive(Clone, Copy)]` 之後下面的程式碼 `assert_eq` 都回過，主要原因是在第一次解引用時，其實做了一次 Copy ，並且將 5 寫入副本裡面，而這個副本的生命週期在這一行之後就結束了，所以這個賦值是無效的，也不會變原本 struct 裡面的內容。
```rust
#[derive(Clone, Copy)]
struct Ttt {
    pub n: usize,
}

let mut st = Ttt {n: 10};
let ptr = &raw mut st;

unsafe { *ptr }.n = 5;
assert_eq!(st.n, 10);

unsafe { &mut *ptr }.n = 5;
assert_eq!(st.n, 5);
```
更多詳細的部分可以參考 [`std::ptr`](https://doc.rust-lang.org/std/ptr/index.html) 和 [Primitive type pointer](https://doc.rust-lang.org/std/primitive.pointer.html)

Rust 裡面的全域變數分成三種： `const` 、 `static` 、 `static mut` ，可以和 C 裡面的 `const` 跟 `static` 對應。其中 `static` 不同於 `const` 通常用在有提供內部可變性並且在程式裡真正會改變其值的變數上，如：`core::atomic` 裡面的原子變數等。為了避免 Race Condition 等平行畫程式會出現的問題，一個形態要被宣告為全域變數在 Rust 裡面必須要有 Sync 這個 trait 才行；然而在 Kernel 開發裡，有很多全域變數是需要在程式開始之後才能初始化（如讀取 Device Tree 、依賴 Memory Allocator 等），這也造成宣告全域變數時需要與編譯器搏鬥一番。對於這個問題，以結論來說，建議使用 [spin](https://docs.rs/spin/latest/spin/index.html) 這個 crate 裡面的 `Once` ，在 core 模式下能用並且能讓變數延遲初始化；或者是使用 `static mut` 形態為 `Option<T>` 並且初始化為 `None` 。[這一篇文章](https://www.sitepoint.com/rust-global-variables/)有講述 Rust 全域變數的宣告跟使用方法，但是不完全適用在 bare metal 的環境，可以斟酌參考。

Send 跟 Sync 這兩個概念是 Rust 給與形態在多線程分享訊息的方法，可以參考 [這裡](https://rust-lang.tw/book-tw/ch16-04-extensible-concurrency-sync-and-send.html) 來複習相關的概念。

# 心得
這一章開始出現許多記憶體操作，像是 Self relocation、Kernel Upload 等，並且在寫入之後會跳過去執行！？雖然之前就知道程式碼存在記憶體裡並且執行，但是這還是第一次「手動」載入程式碼，與之前只有操作記憶體的「讀取」跟「寫入」完全不一樣，`pc+4` 這個在計算機組織裡面寫過的電路，在這裡變成了實際操作的現實。不過這也給我留下了一個疑問：之前學習的「一個程式碼的 text 部分是唯獨的」以及 linker file 裡面，有程式碼屬性（`rwx`）的設定，這些好像沒什麼用？這些疑問到了 virtual memory 才獲得解答。
