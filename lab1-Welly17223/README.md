# Lab 1
[作業聯結](https://nycu-caslab.github.io/OSC2026/labs/lab1.html)
[Exercise](https://reurl.cc/dq6992)

## Rust 環境建立

Lab 0 介紹的是以 C 為主的環境，建立好之後，要跑 Rust 還要有其他的設定。

1. 用 `rustup` 安裝 RISC-V 相關的 tool chain ： `rustup install riscv64gc-unknown-none-elf riscv64imac-unknown-none-elf`
2. 在 work directory 下面建立 `.cargo` 資料夾，並且寫入編譯設定：
```toml
[build]
target = "riscv64imac-unknown-none-elf"
```
3. 在 `main.rs` 裡面需要標注 `#![no_std]`、`#![no_main]` 標示不使用 `std` 並且沒有傳統意義上的 main 函數（需在 `start.s` 裡面呼叫）。
4. 自訂 panic handler 。會在程式 panic 時呼叫這個函數。一開始可以簡單這樣寫，後面在根據需要在 panic 時印出對應的訊息。(註：我在 Lab 6 之後才寫好比較 方便除錯的 panic handler ，如果參考可以飛過去)
```rust
#[panic_handler]
fn panic(_panic: &PanicInfo<'_>) -> ! {
    loop {}
}
```
5. 除錯：在 `qemu` 虛擬機執行時加上 `-S -s` 這兩個參數，並且在 `rust-gdb` 或是 `rust-lldb` 裡面用 `target remote:1234` 連結並且除錯。
6. 在除錯時可以以 `debug` 模式編譯，缺點是程式體積較大並且速度較慢（有點類似 c 的 `-O1`，編譯器只會對變數做最保守的推測），在確定沒問題時再用 `--release` ，讓程式在執行時速度變快。
   Rust 的 release 模式有一些小坑，如果在使用某些需要 volatile 的變數時沒有正確使用 `read_volatile`、`write_volatile` 會導致編譯器無法編譯出正確的程式碼，然後跑不起來。雖然 C 也會有一樣的問題，但是如果使用 rust 的 debug 模式時似乎不會對變數記憶體存取上做過多的推測，導致行為正常，但是一使用 release 模式就出事了，因此正確使用 volatile 是很重要的。
7. 撰寫 `build.rs` 讓 rust 編譯器使用我們自己寫的 linker file 以及將 `start.s` 也編譯進去（不過這裡好像是用 `global_asm!(include_str!("start.s"));
`）把 `start.s` 包含進編譯的 text 裡面就是了。差別是使用 rust build dependency `cc` 在 debug 時可以比較方便看到組合語言。 
`build.rs`：
```rust
fn main() {
    println!("cargo:rustc-link-arg-bin=rust-version=-Tlink_script.ld");
    cc::Build::new()
        .file("src/sbi.c")
        .target("riscv64gc-unknown-none-elf") // 指定目標架構
        .compiler("riscv64-elf-gcc") // 指定你的編譯器
        .compile("sbi"); // 將編譯結果命名為 libuart.a
    println!("cargo:rerun-if-changed=linker.ld");
    println!("cargo:rerun-if-changed=src/sbi.c");
    println!("cargo:rerun-if-changed=src/start.s");
}

```
`Cargo.toml` 要記得加入 build dependency `cc` 跟 panic handler：
```toml
[build-dependencies]
cc = "1.0"

[profile.dev]
panic = "abort"

[profile.release]
panic = "abort"
```

> 註：`riscv64gc-unknown-none-elf` 這個架構是在後面的作業改用的編譯器，主要差別在與支援更多 RISC-V 的 extension instruction。
小撇步：我是後來才發現的，全域變數如果需要在程式開始之後才初始化，可以宣告為 `spin` 這個 crate 裡面的 `Once` 這個形態，寫起來比較方便（參考 Lab 6、Lab 7）。

## 超級重要
務必參考[這篇文章](https://zhuanlan.zhihu.com/p/343688629)來了解 C 裡面 volatile 的用法，基本上 Rust 也適用。後面進入到有 Context Switch 時如果遇到奇怪的錯誤可能會是因為沒有 volatile 所導致的！不過也要搞清楚與 atomic 的使用時機。

> 註：如果有必要，也可以使用別人包裝好的 volatile crate ，簡化 volatile 需要的 unsafe 操作。

## Basic Initialization

在開始之前，可以先看上課投影片以及[這個](https://wen00072.github.io/blog/2014/03/14/study-on-the-linker-script/)連結熟悉 linker file ，並且完成 exercise。

### `start.s` 以及 linker file

首先是撰寫 `start.s`，是系統開機之後，在進入 main 函數之前需要執行的程式。要寫的部分包含：將 `bss` 區域設定為 0（C 語言的定義，未初始化的全域變數必須為 0）、進入 Kernel。在執行時 `.bss` 的區域可以透過在 linker file 裡面定義 label 並且用 RISC-V 的 pseudo instruction `la` 來將地址讀出來；初始化完之後再跳到 main 函數。

> 註：`la` 這個指令會根據目前的 `pc` （program counter）做偏移來算出對應 label 的記憶體位置。

Linker file 要這樣寫，首先是將 `.text.boot` 這個在 `start.s` 裡面定義的 section 在 link 時放到程式的最前面，以確保在開機初始化之後跳到程式所在的位置時，這一段程式可以被第一個執行；之後依序放入 `.text`、`.data`、`.rodata`、`.bss` 以及最後面的 stack。（註：雖然程式裡面的這些 section 預設都是 8 bytes aligned 的，不過最好在 linker file 裡面也自己對齊一下以確保不會出錯）

大坑：如果這裡的 stack 給得太小，在後面 Lab 裡面 local variable 越來越多之後，有可能會 stack overflow ，而且由於程式在這裡是不會做邊界檢查的，所以很有可能蓋到前面的其他 section 而不自覺，十分危險；所以這裡 stack 大小建議可以給大一點（如 2 MB 之類的），4 KB 確實有點太小了。

## UART Setup

Exercise 裡面有提供 QEMU 的 UART address ，這一個作業是要去查 Orange Pi RV2 的 Spec 並且讓 UART 能夠在板子上跑。在這個 Lab 裡面會使用這些 UART 的記憶體：
- Receive Buffer Register(RBR)
- Transmit Holding Register(THR)
- Line Status Register(LSR) 的 Data Ready(DR) 跟 Transmit Data Request(TDRQ) 這兩個 bit。
UART 的輸入就是等 Data Ready 為 1 之後讀取 RBR ，等 TDRQ 為 1 之後寫入 THR。這裡建議可以建立 UART struct ，並且 `impl core::fmt::Write`，這樣可以使用 format output 了（可以參考 Lab 2 的程式碼）。在 Lab 2 之後會有方式讓程式不用將硬體 Address Hard Code 在程式碼裡面，所以這個 Lab UART 一些程式碼在之後要改。

小坑：UART 是所讀取的是 MMIO 記憶體，因此需要 volatile ，在 release 模式下才不會出問題。
小坑：這臺 Orange Pi 上有很多個 UART 裝置，記得要找 debug 孔的那一個。

## Simple Shell
簡單的字串處理，沒什麼好說的。建議可以在這裡處理 backspace 的輸入，才不會打錯就出問題。

## System Information
Open SBI 相關知識可以看上課講義，或是這兩個連結：[1](https://zhuanlan.zhihu.com/p/1924502497390736107)、[2](https://ithelp.ithome.com.tw/articles/10290939)。

> PS:上面的第一個連結包含了助教程式碼的解析， Demo 前最好看過，助教會問相關問題。

透過 Open SBI 開機 （M Mode），並且進入 S Mode 的 Kernel 之後，如果要取用一些硬體的功能，就必須使用 `ecall` 將參數傳給 M Mode 的 Open SBI 然後讀取回傳值。我的程式一開始因為嫌麻煩，所以沒有自己寫，直接用了 exercise 裡面的程式，如果要在 Rust 裡面直接寫，而非另外 Call C 語言的 Function 可以參考以下程式：

```rust
#[repr(C)]
struct SBIRet {
    error: core::ffi::c_long,
    value: core::ffi::c_long,
}

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sbi_ecall(
    a0: usize,
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    fid: usize,
    ext: usize
) -> SBIRet {
    core::arch::naked_asm!(
        r#"
        ecall
        ret
        "#
    );
}
```

解析：Rust 的 naked function 可以當成 inline 組語看待，其中重要的差別是，在執行時因為是 function call ，所以編譯器會先儲存 caller saved register 並且在回傳時自動恢復；在這個 function 裡面只能寫組合語言，並且如果要使用 callee saved register 要手動儲存、自己寫 `ret` 基本上就跟寫組合語言是一模一樣的 。函數在這個函數裡，傳入的參數會依順序存在 `a0`~`a7` ，因此可以直接呼叫 `ecall`；呼叫 `ecall` 之後，會進入 M Mode ，並且 Open SBI 會讀取 `fid`、`ext` ，來決定要執行嗯什麼程式，最後將回傳的 error 寫入 `a0`、 value 寫入 `a1`，而 C 語言在回傳一個 struct 時，若是 struct 小於兩個變數，會將兩個變數寫入 `a0` 跟 `a1`；由於這個 function 標注了 `extern "C"`，因此 Rust 會以 C 語言的標準去處理這個 function。

