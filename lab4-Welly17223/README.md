# Lab 4: Interrupt

[作業連結](https://nycu-caslab.github.io/OSC2026/labs/lab4.html)

[Exercise](https://reurl.cc/yOYYDO)

[RISC-V Privileged](https://riscv.github.io/riscv-isa-manual/snapshot/spec/#vol:priv)

[PLIC]

[PLIC ch intro]

[PLIC ch intro]:  https://ithelp.ithome.com.tw/articles/10277204?sc=hot "PLIC 外部中斷機制"
[PLIC]: https://ithelp.ithome.com.tw/articles/10277204?sc=hot "RISC-V PLIC spec"
 
這一章的許多操作與硬體有關，因此必須多看一些硬體的說明書（如 PLIC 以及 RISC-V 等），除錯重點通常也是忘記設定 interrupt bit 或是記憶體位置搞錯，這些點上可以多加注意。

首先先附上兩個基本的 64 bits `CSR` 的 bit field：

`sstatus`：

|Bit|Name|
|---|---|
|0|`WPRI`|
|1|`SIE`|
|2-4|`WPRI`|
|5|`SPIE`|
|6|`UBE`|
|7|`WPRI`|
|8|`SPP`|
|9-10|`VS[0:1]`|
|11-12|`WPRI`|
|13-14|`FS[0:1]`|
|15-16|`XS[0:1]`|
|17|`WPRI`|
|18|`SUM`|
|19|`MXR`|
|20-22|`WPRI`|
|23|`SPELP`|
|24|`SDT`|
|25-31|`WPRI`|
|32-33|`UXL[0:1]`|
|34-62|`WPRI`|
|63|`SD`|

`sscause`：

|Bit|Name|
|---|---|
|0-62|Exception Code|
|63|Interrupt|

|Interrupt|Exception Code|Description|
|---|---|---|
|1|0|Reserved|
|1|1|Supervisor software interrupt|
|1|2-4|Reserved|
|1|5|Supervisor timer interrupt|
|1|6-8|Reserved|
|1|9|Supervisor external interrupt|
|1|10-12|Reserved|
|1|13|Counter-overflow interrupt|
|1|14-15|Reserved|
|1|≥16|Designated for platform use|
|0|0|Instruction address misaligned|
|0|1|Instruction access fault|
|0|2|Illegal instruction|
|0|3|Breakpoint|
|0|4|Load address misaligned|
|0|5|Load access fault|
|0|6|Store/AMO address misaligned|
|0|7|Store/AMO access fault|
|0|8|Environment call from U-mode|
|0|9|Environment call from S-mode|
|0|10-11|Reserved|
|0|12|Instruction page faults|
|0|13|Load page fault|
|0|14|Reserved|
|0|15|Store/AMO page fault|
|0|16-17|Reserved|
|0|18|Software check|
|0|19|Hardware error|
|0|20-23|Reserved|
|0|24-31|Designated for custom use|
|0|32-47|Reserved|
|0|48-63|Designated for custom use|

`CSR` 的相關指令可以參考[這一篇文章](https://ithelp.ithome.com.tw/articles/10290501)。

Rust 有提供 `riscv` 這個 crate ，可以用來操作 `CSR` 等 RISC-V 上獨有的功能。建議可以引入，減少人為錯誤，並且讓程式可讀性增加。在 `Cargo.toml` 裡面加入這一行來使用這個 crate 的 supervisor mode 的功能：
```toml
[dependencies]
riscv = { version="0.16.0", features = ["s-mode"] }
```

## 先備知識 Rust 的 Function Pointer 以及 Function Trait

> Function Trait 施工中…… ，應該對於這些 Lab 影響不大。

`fn` 是 Rust 裡面的一種 Type ，基本上等價於 C 語言裡面的 Function Pointer 。寫法如下：`fn(usize, usize) -> i32` ，透過這種方法，就可以獲得一個 function pointer 。在 Rust 裡面，必須透過將 function pointer 轉換成 `*const ()` 之後才能轉換成 `usize` 等形態，不能直接轉換成 `usize` 。

## Exception

要啟動 interrupt 首先需要設定寄存器 `stvec` ，將 interrupt 的 function 地址（virtual 或是 physical 皆可）儲存到這個寄存器裡面，並且要求 2 bits aligned 。最低位的 0-1 個 bits 是 mode ，如果為 0 則是所有 interrupt 都跳到這個地址，如果是 1 則是跳到 $\text{base address} + 4 \times \text{cause}$ 。範例寫法如下：

讀取地址並且寫入 `stvec` 暫存器：
```asm
la t0, your_function_name
csrw stvec, t0
```

設定第 0 個 bit 為 1 （vectored mode ）：
```asm
li t0, 0x1
csrrs zero, stvec, t0
```

Function 用 `.align 2` 來規範 2 bits aligned 。Mode 0 的模式下：
```asm
.global handle_exception
.align 2
handle_exception:
    ; Save context
    ; call rust functino
    ; Restore context
    sret
```

Mode 1 的模式下：
```asm
.global handle_exception
.align 2
handle_exception:
    j exception_handler
    .p2align 2
    j supervisor_software_handler
    .p2align 2
    j reserved_handler
    .p2align 2
    j reserved_handler
    .p2align 2
    ; ...
    j supervisor_timer_handler
```

最後定義「能被組合語言呼叫」的函數，如同 Lab 1 的 main 一樣，並且接收組合語言裡面儲存的 context 。
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(C)]
pub struct pt_regs {
    pub ra: usize,
    pub sscratch: usize,
    pub gp: usize,
    pub tp: usize,
    pub t0: usize,
    pub t1: usize,
    pub t2: usize,
    pub s0: usize,
    pub s1: usize,
    pub a0: usize,
    pub a1: usize,
    pub a2: usize,
    pub a3: usize,
    pub a4: usize,
    pub a5: usize,
    pub a6: usize,
    pub a7: usize,
    pub s2: usize,
    pub s3: usize,
    pub s4: usize,
    pub s5: usize,
    pub s6: usize,
    pub s7: usize,
    pub s8: usize,
    pub s9: usize,
    pub s10: usize,
    pub s11: usize,
    pub t3: usize,
    pub t4: usize,
    pub t5: usize,
    pub t6: usize,
    pub sepc: usize,
    pub sstatus: usize,
    pub scause: usize,
    pub stval: usize,
}

#[unsafe(no_mangle)]
extern "C" fn do_trap(regs: *mut pt_regs) {}
```

設定完之後，要接著在 interrupt 裡面完成 `sscause` 的判斷以及處理。在 Lab 5 之後，因為 interrupt 有點多，因此我宣告了一個 trait 用來作為處理 interrupt 的統一界面。處理 U-mode 的 interrupt 之後，要將 `sepc` 加上 4 是因為 U-mode 使用 `ecall` 來產生 interrupt ，此時的 `sepc` 會是 `ecall` 這一個 instruction 的地址，由於 kernel 處理完了，因此要跳到下一個 instruction 的位置而不是 `ecall` 這一個 instruction。

### Mode Switch: S-mode to U-mode

要切換成 user mode ，需要
1. 從 ram disk 讀取助教給的程式到 dynamic memory 裡面
2. `sstatus.SPP` 設定為 0
3. `sstatus.SPIE` 設定為 1
4. `sepc` 設定為對應的 address
5. `sscratch` 設定為此 process 的 kernel stack
6. 用 `sret` 返回 user mode 。

> 註：以上關於 `CSR` 的操作都可以用 `riscv` crate 達成，詳情可以直接看 code 。在這一個作業裡面，由於沒有 scheduler ，因此執行助教給的程式之後，就會進入無限循環。

要小心 kernel thread 要設定的大一點，不要 stack overflow ，否則又是 debug 一輩子。

## Core Timer Interrupt

這個作業基本上沒有什麼難的地方，除了助教在作業裡面寫的流程以外記得
1. 從 device tree 裡面找到 CPU 的 frequency （`/cpus/timerbase-frequency`）。
2. 要記得將 `sip.STIP` 清除並且設定下兩秒的 timer 。

如果 CPU 支援 `sstc` 這個 ISA extension ，設定 timer 時可以不用使用 SBI `ecall` 而是用 `CSR` 寫入 `stimecmp` 這個寄存器。

## Orange Pi RV2 UART0 Interrupt

建議可以看過 [PLIC] 、 [PLIC ch intro] 以及上課講義熟悉 PLIC 相關背景知識。

這裡的 external interrupt 照著上面給的 Spec 、助教給的步驟寫就好，這裡就不再特別說明。至於 UART 則是要在 interrupt 觸發的時候將讀取到的資料儲存到 queue 裡面等待 process 讀取，並且設定 UART 的 `ier`、`mcr` 寄存器。當 UART 中斷發生時，讀取 `iir` 寄存器來分辨是哪一種 interrupt 被觸發了，並且在處理完 interrupt 之後，將 IRQ 寫入 claim 的記憶體裡面。我在這裡的設計 UART 在初始化的時候將本身所代表的 IRQ 儲存到 IRQ table 裡面，並且在 interrupt 觸發時去 IRQ table 裡面尋找這個 `scause` 對應到哪一種的外部中斷。

## Timer Multiplexing

建立一個 priority queue 依據發生的時間由小排到大，用來儲存還未發生的 timer ，在每次 `add_timer` 呼叫的時候或是 timer interrupt 被觸發時設定時間最接近的 timer interrupt 為下一次 interrupt 發生的時間。此時用 Rust 就很方便了，可以直接使用 `BinaryHeap` 作為 priority queue 。時間表示上，可以考慮使用 Rust 提供的 `core::time::Duration` （我在寫的時候不知道），在設定硬體中斷時再把它轉換成 ticks 。

在實做助教要求的 API 要注意的點是 callback function 需要處理 NULL pointer 的情況，此時代表僅啟動中斷，而不執行任何程式。我這裡的實做是當 callback 為 NULL 時，傳入 Empty 函數，後來想想比較好的做法應該是將 Timer Entry 的 Function 以 `Option<T>` 的形式傳入。
```c
//An example API
void add_timer(void (*callback)(void*), void* arg, int sec){
    ...
}
```

由於此 Function 是在 Timer interrupt 時被執行，有可能與呼叫 `add_timer` 的 Function 不共享同一個 stack ，因此傳入的參數必須儲存在由 kernel allocator 動態分配的記憶體之中。
```rust
pub struct TimerEntry {
    pub f: fn(*const u8),
    pub args: *const u8,
    time: u64,
    // unit is raw
    repeat: Option<Time>,
}
```

後來仔細想想，可以為 `TimerEntry` 跟下面的 `TaskEntry` 要執行的 callback function 以及其 arguments 獨立建立一個 struct ，在這兩個 struct 裡面定義 `Option<CallbackManager>` ，並且用這一個 `enum` 管理 Arguments 在 Heap 裡面的所有權，這樣可以更靈活的複用以及管理 callback function 和 arguments ，也可以減少在 Rust 裡面出現 NULL pointer 等指標的問題。

```rust
pub trait Callback {
    fn callback(&self);
}

enum CallbackManager {
    C {
        f: unsafe extern "C" fn(*const ()),
        arg: *const (),
    },
    Rust { args: Box<dyn Callback> },
}

impl CallbackManager {
    pub fn from_rust(args: Box<dyn Callback>) -> Self {
      Self::Rust { args }
    }

    pub fn from_c(f: unsafe extern "C" fn(*const ()), arg: *const ()) -> Self {
        Self::C { f, arg }
    }

    pub fn call(&self) {
      match self {
        Self::Rust { callback } => callback.callback(),
        Self::C { f, arg } => unsafe { f(*arg) },
      }
    }
}

unsafe impl Sync for CallbackManager {}

pub struct TimerEntry {
    pub callback: Option<CallbackManager>,
    time: u64,
    // unit is raw
    repeat: Option<Time>,
}
```

## Concurrent I/O Devices Handling

這一個 Advanced Exercise 基本上完全不影響後面的 Lab ，如果時間不夠可以跳過，否則根據實做方法，有可能會花很多時間在除錯上。我一開始是使用類似 Context Switch 的實做方法來儲存 preemption 之前的 Context ，搞了好久都沒搞出來，後來才想到現在這個簡潔的處理方法：
1. 建立一個全域變數用來記錄目前處理的 I/O Devices Handling 的 priority。
2. 當 Interrupt 要結束時判斷目前 Queue 裡面是否有比當前 Task priority 更高的 Task 。
3. 如果有則
    1. 從 Queue 裡面取出 Task
    2. 儲存先前 Task 的 priority 、將 priority 設定為當前 Task 的 priority 。
    3. 開啟 Interrupt 並且執行 Task ，
4. 當 Task 結束時重複這個流程直到沒有 priority 更高的 Task。

這裡一樣要考慮 C 的兼容性問題。
```c
void add_task(task_callback_t callback, void *arg, int priority) {
  ...
}
```

為了兼容 C 語言， callback 接收的參數是一個 pointer ，在 Rust 裡面必須重新考慮所有權的問題，我的做法是將參數的所有權交給 `TaskEntry` 處理，當 `TaskEntry` Drop 時會判斷此 Timer 是否是由 Rust 建立，如果是那麼就會回收，至於由 C 語言建立的 Timer 則是交由 C 語言的呼叫者自己管理。

```rust
pub type TaskCallback = extern "C" fn(*const u8);

enum Args {
    CArgs,
    RustArgs,
}

pub struct TaskEntry {
    callback: TaskCallback,
    args: *const u8,
    args_type: Args,
    priority: u32,
    id: u64,
}
```

改成 `CallbackManager` 之後的版本：

```rust
pub struct TaskEntry {
    callback: CallbackManager,
    priority: u32,
    id: u64,
}
```

## 補充 Early Interrupt Handler

在所有元件都還沒初始化之前，可以先設定一個簡單的 interrupt handler 到 `stvec` 上面，當出現 interrupt bit 為 0 的錯誤時印出錯誤訊息（例如，存取到空的記憶體或是沒有 aligned 的記憶體等）。這一個 handler 可以簡單的印出 `sscause` 、 `stval` 、 `sepc` 即可，接著可以用 `addr2line` 工具將對應的 `sepc` 轉換成程式碼裡面的行數或是設定 GDB 的中斷點在這個函數來定位錯誤。

## 心得

這一次的作業最需要注意的點就是寄存器的各個 bits 有沒有設定正確， `sstatus` 、 `sie` 等寄存器要啟動中斷， `plic` 控制器要設定 threshold 以及啟動中對應 hart 的外部中斷、 `sip` 的 bit 要記得清除…… 等等族繁不及備載；反過來說，也就是將這些 `csr` 的功能搞懂之後，其實應該是不用耗很多時間就可以寫出來的。在已經碰過前三個 Lab 的情況下來做這個 Lab 確實是有一種鬆了一口氣的感覺，算是學期間的小小休息週吧，如果寫完有時間，可以趕緊開始做接下來的兩個十分困難的 Lab ！
