# Lab 5 Thread and User Process

[作業連結](https://nycu-caslab.github.io/OSC2026/labs/lab5.html)

[Exercise]

[Process and Thread 上課投影片](https://people.cs.nycu.edu.tw/~ttyeh/course/2026_Spring/IOC5226/slide/process.pdf)

總之還是先上那一張很經典的 Thread state 圖。這一次的作業就是很經典，在作業系統課程上都學過的： Thread and User process ，要從建立 Thread 到管理狀態、處理 System Call 以及最後 Thread 的回收等等。另外為了確定 context switch 有正確執行，助教還另外要求用 HDMI 播放影片，不過這個部分很多的程式碼都由助教完成了（Exercise 2 的部分），照著抄便是。

![Thread Status](https://www.cs.uic.edu/~jbell/CourseNotes/OperatingSystems/images/Chapter3/3_02_ProcessState.jpg)

## 原子變數以及原子操作

既然進入到了多個 process 交互執行的場景，那麼自然有可能會用到 `Mutex` 、 `atomic` 等維持多進程之間資料同步的結構。雖然這一次作業僅僅要求單核心模式下的作業系統，不過如果有意想要繼續寫下去的話，可以多流個心眼，同時也是為了完成 Rust 對於 immutable 全域變數的要求。不然其實在單核心下，使用 volatile 讀取、寫入就可以了。 

[這一篇文章](https://zhuanlan.zhihu.com/p/6403936954) 對於原子操作的不同種類的 Memory Order 寫的十分詳細，如果想要進一步改善系統效能或是不知道要用哪一個原子操作 Memory Order 的可以參考。

## Thread

作業大魔王，基本上寫完這個作業就完成一半了，為了完成這一項，你必須寫完：
- Idle Thread
- Scheduler
    - Context Switch
    - Thread Queue
    - Zombie Thread Collection
- Thread Control Table
- Waiting Queue
- Thread State Control

又是一個漫長的除錯過程……。

[Exercise]: https://reurl.cc/R2LGbZ

### Creating a Thread

要建立 thread ，先從定義 `TCB` 開始。以下是在這個 Lab 根據需求慢慢加上去之後的最終版本的 `TCB` ， `Context` 負責儲存這一個 process 的 register ，由於在呼叫 `switch_to` 函數做 context switch 的時候， `caller saved register` 會被儲存起來，因此這裡只要保存 `callee saved register` 和 `ra` 即可。另外我還另外儲存了 awake time ，讓 thread 可以在執行時間到之後切換成另外一個 thread 。 `stack` 用來儲存 user stack ，如果是 kernel thread 就不需要，因此用 `Option` ，而 `kernel_stack` 則是每個程式必須的。 `parent` 這一個 field 是用來儲存 `parent` 的 `TCB` ，特別提起是因為後來想想應該要用 `alloc::sync::Weak` 來避免循環引用以及占用 `TCB` 所有權的，如此一來就不用用 `Option` 了。

```rust
pub type SafeSendTCB = Arc<SpinLock<ThreadControlTable>>;

#[derive(Default, Clone, Copy)]
#[repr(C)]
pub struct Context {
    pub ra: usize,
    pub sp: usize,
    pub s: [usize; 12],
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum State {
    New,
    Ready,
    Waiting,
    Running,
    Terminate,
}

#[derive(Clone)]
#[repr(C)]
pub struct ThreadControlTable {
    pub context: Context,
    pub user_init_sp: usize,
    pub awake_time: u64,
    pub exit_code: isize,
    pub state: State,
    pub pid: u32,
    pub ppid: u32,
    pub parent: Option<schedule::SafeSendTCB>,
    pub children: Box<BTreeMap<u32, schedule::SafeSendTCB>>,
    pub term_children: Box<BTreeMap<u32, schedule::SafeSendTCB>>,
    pub kernel_stack: Box<[u8]>,
    pub stack: Option<Box<[u8]>>,
    pub reschedule: bool,
    pub sig: Box<SigAct>,
}
```

<p>宣告好了 thread control table 之後，就是<ruby>建立新 thread <rt>初始化變數</rt></ruby>了。一開始，可以先建立 kernel shell 的 thread ，這裡使用<code>unsafe { Box::<[u8; 0x10000]>::new_zeroed().assume_init() }</code>的原因是因為如果在 Rust 的 debug 模式下，使用 <code>Box::new([u8; 0x10000])</code> 會現在 stack 建立 <code>[u8; 0x10000]</code> 之後再複製過去，造成 stack overflow ，當初也是花了好久才找出錯誤。有一些函數在 thread 被建立時需要設定，像是 <code>sepc</code>、<code>sstatus</code> 以及 thread 被結束之後必須將自身的狀態改成 Terminal 等……，這裡可以利用 <code>switch_to</code> 會恢復 context 的特性，將 <code>ra</code> 設定成 <code>init_thread</code> 並且將特定寄存器設定成變數，並且最後開始執行 thread 。像這樣：</p>

```rust
/// init_thread
/// # Safety
///
/// threadinit
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn init_thread() {
    core::arch::naked_asm!(
        r#"
            csrw sepc, s0
            csrw sstatus, s1
            csrw sscratch, s2
            mv ra, s3
            // check the return mode if is u mode, then switch the kernel stack and user stack
            andi s1, s1, (1 << 8)
            bne s1, zero, s_mode
            csrrw sp, sscratch, sp
        s_mode:
            sret
        "#
    );
}

impl ThreadControlTablk {
    pub fn new(func: *const (), ppid: u32, sstatus: usize) -> Self {
        let kernel_stack = unsafe { Box::<[u8; 0x10000]>::new_zeroed().assume_init() };
        let kernel_stack_top_ptr =
            kernel_stack.as_ptr().wrapping_byte_add(kernel_stack.len()) as usize;

        let mut s = [0; 12];
        let pid = alloc_pid();
        s[0] = func as _;
        s[1] = sstatus;

        // Alloc kernel stack for u mode stack
        let (stack, stack_top_ptr) = if sstatus & (1 << 8) == 0 {
            schedule::USER_THREAD_COUNT.fetch_add(1, atomic::Ordering::Relaxed);
            let stack = unsafe { Box::<[u8; 0x10000]>::new_zeroed().assume_init() };
            let stack_top_ptr = stack.as_ptr().wrapping_byte_add(stack.len()) as usize;
            s[2] = stack_top_ptr;
            s[3] = u_mode_do_exit as *const () as _;

            (Some(stack as _), stack_top_ptr)
        } else {
            s[3] = do_exit as *const () as _;
            (None, 0)
        };

        Self {
            context: Context {
                ra: init_thread as *const () as _,
                sp: kernel_stack_top_ptr,
                s,
            },
            state: State::New,
            children: Box::new(BTreeMap::new()),
            term_children: Box::new(BTreeMap::new()),
            parent: Some(schedule::curr_thread_arc()),
            ppid,
            awake_time: 0,
            pid,
            exit_code: 0,
            kernel_stack,
            stack,
            user_init_sp: stack_top_ptr,
            reschedule: false,
            sig: Box::new(SigAct::default()),
        }
    }
}
```

這裡有點過度設計了，使用這個 `new` 函數時，應該只會用在建立 kernel thread 的狀況，基本上不用考慮有 `sstatus.SPP` 為 0 的情況，畢竟建立 user thread 必須要將 `sepc` 設定在某個已經讀到記憶體的地方，另外寫一個函數還是比較簡單的。

總之，建立並且調度一個 kernel thread 的流程就是：
1. 建立 thread control table
2. 經過 scheduler 之後跳到 `thread_init` 函數裡面
3. 初始化必要的寄存器 `sstatus`、`sepc`、`sscratch`、`ra`、`sp` 等並且使用 `sret` 進入 thread

### Scheduler and Context Switch

![Round-Robin](https://media.tenor.com/9Zmv80efA8cAAAAd/round-robbin-bird.gif)

Scheduler 負責調度，尋找下一個可以執行的程式並且切換過去、將現在正在執行的程式的 context 儲存並且如果程式是 running 的狀態就將此程式改成 ready 並且丟到 ready queue 裡面，如果是 terminal 的狀態則做清理。清理流程包含：
- 將還活著的 child process 丟給 idle thread ，並且設定 child process 的 parent process 為 idle thread。
- 清理已經 terminal 的 child process 。
- 如果這個 process 正在被其他的 thread wait ，那麼則將這個 thread 喚醒。
由於這個作業只要求最簡單的 round-robin ，因此用一個 FIFO queue 就可以了。

Context switch 簡單來說就是將一個 thread 的 context 儲存起來，並且將另一個 thread 的 context 讀取出來，這裡的 offset 數字必須根據 `ThreadControlTable` 裡面的各個 field 來寫：
```rust
/// context switch
/// # Safety
///
/// This function only use in schedule function to execute context switch
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn switch_to(prev: *mut Context, next: *mut Context) {
    arch::naked_asm!(
        "rdtime a2",
        "sd ra , 8 *   0(a0)",
        "sd sp , 8 *   1(a0)",
        "sd s0 , 8 *   2(a0)",
        "sd s1 , 8 *   3(a0)",
        "sd s2 , 8 *   4(a0)",
        "sd s3 , 8 *   5(a0)",
        "sd s4 , 8 *   6(a0)",
        "sd s5 , 8 *   7(a0)",
        "sd s6 , 8 *   8(a0)",
        "sd s7 , 8 *   9(a0)",
        "sd s8 , 8 *  10(a0)",
        "sd s9 , 8 *  11(a0)",
        "sd s10, 8 *  12(a0)",
        "sd s11, 8 *  13(a0)",
        // save current time
        "sd a2 , 8 *  15(a1)",
        "ld ra , 8 *   0(a1)",
        "ld sp , 8 *   1(a1)",
        "ld s0 , 8 *   2(a1)",
        "ld s1 , 8 *   3(a1)",
        "ld s2 , 8 *   4(a1)",
        "ld s3 , 8 *   5(a1)",
        "ld s4 , 8 *   6(a1)",
        "ld s5 , 8 *   7(a1)",
        "ld s6 , 8 *   8(a1)",
        "ld s7 , 8 *   9(a1)",
        "ld s8 , 8 *  10(a1)",
        "ld s9 , 8 *  11(a1)",
        "ld s10, 8 *  12(a1)",
        "ld s11, 8 *  13(a1)",
        "mv tp, a1",
        "ret"
    );
}
```

在這裡有可能會遇到 `Mutex` deadlock ，此時應該多加留意當前的 thread 進入 `scheduler` 函數之前有沒有持有某些 lock ，這些 lock 必須被 `drop` 並且在 `scheduler` 函數結束後再重新取得 lock 。另外如果變數沒有使用 volatile 讀取寫入或是 atomic 會造成 context switch 之後可能的讀取錯誤（我有在 UART 裡面的 `RingBuf` module 裡面遇到，應該是因為<ruby>變數存在寄存器<rt>沒有 v o l a t i l e </rt></ruby>裡面）此時應該多加注意這些特殊變數的使用時機。

Scheduler 完成之後，應該要為各種會 block 的裝置設定 waiting queue ， `waitpid` 的 thread 自己也要一個。以 UART 舉例，在 `pop_rx` 以及 `push_tx` 時分別在 `rx` queue 裡面沒東西以及 `tx` queue 滿時進入 waiting ；此時，就要把目前的 thread 放到各自的 waiting queue 裡面，並且將狀態設定為 wait ，並且等待 UART interrupt 時 `rx` 或是 `tx` queue 準備好之後取出。

### The Idle Thread

`pid` 為 0 或是 1 的 thread ，一個系統一定要有的最基本的 thread ，同時要負責清除已經 terminal 的 child process 。基本上沒什麼好說的，頂多就是因為其特殊性，因此要在初始化 scheduler 時手動建立。另外如果不希望 QEMU 或是 Orange PI 在執行時不希望 <ruby>CPU 都是滿載<rt>浪費電</rt><ruby>，也可以在這裡下一點功夫， Linux kernel 有許多對於 idle thread 的省電策略可以參考。最簡單的策略就是加入 [`wfi` 指令](https://doc.nucleisys.com/nuclei_spec/isa/lowpower.html)，讓 CPU 在有 interrupt 時才喚醒。

```rust
pub fn idle_thread() -> ! {
    let init_arc = schedule::get_init_thread();
    loop {
        let lock = init_arc.lock();
        lock.get_mut().term_children.clear();
        drop(lock);

        let disable = SModeInterrupt::new();

        if get_process_ready_queue().is_empty() {
            unsafe {
                asm!("wfi");
            }
        }

        drop(disable);

        schedule::schedule();
    }
}
```

### End of a Thread

將程式的狀態改成 Terminal 並且由 scheduler 回收這個 thread 的記憶體等。如果是在 user mode 下，則是呼叫特定的 system call 進入 kernel mode 來處理。如何讓 thread 在結束之後進入這個函數呢？在 `thread_init` 設定 `ra` 到 `do_exit` 即可。不過在 virtual memory 下的 user mode 可能會需要做額外的設定。

> 註：有時程式會在某些特別的地方 exit ，導致程式本身的 `TCB` 還處在 lock 的階段，因此應該要在 `do_exit` 強制解鎖，以防止後面在 scheduler 處理時 deadlock 。主要會發生在後面 Lab 6 的某些時候。

## User Process and System Call

### User Process

雖然一般來說 process 長成下圖的樣子，不過那是在啟動 virtual memory 以及讀取 ELF 檔案時才長這個樣子。在這個作業裡面，只要分配一個空間讓 process 可以跑就好了，因此會與這個經典的架構有點不太一樣。

![Process memory layout](https://d8it4huxumps7.cloudfront.net/uploads/images/6851382354377_memory_layout_in_c_inside.jpg?d=2000x2000)

建立 User process 的流程基本上與建立 thread 是差不多的，以目前的狀況來說，一個 thread control block 與一個 process 是一樣的，除了要先讀取 user process 到 stack 裡面以及記得 `fence.i` 之外，以同樣的方式建立就好。不過要注意設定 `sstatus.SPP` 為 0 讓程式 `sret` 之後可以進入 user mode 。我在寫這個作業時有想到如果一個 process 包含多個<ruby> thread <rt>執行緒</rt></ruby>時要如何以單一 thread control table 處理呢？雖然在這個作業裡不需要考慮那麼多，不過如果想要深入了解可以查看 [sysprog/Linux 核心設計: 不只是執行單元的 process](https://hackmd.io/@sysprog/linux-process) 。

### System Call

在上一個作業裡面已經有將 system call 的資訊印出來的程式了。在這裡，就是要把作業要求的各種功能寫出來。有些像是 `getpid` 的 system call 相對簡單就略過了，接下來只記錄比較複雜的 system call 。

順帶一提一個實做細節，在之前的程式碼裡面，由於將 `pt_regs` 裡面的 field 都宣告成了 `usize` ，因此如果 system call 的返回值是負數時，可以使用 `-1_isize as usize` 這樣的做法來將負數轉換成 `usize` 的形態寫入 `a0` 寄存器裡面。

#### `uart_read`、`uart_write`

單獨拿出來說是因為這裡使用的 `pop_rx` 以及 `push_tx` 如果 queue 裡面沒有東西就會觸發 context switch ，要格外注意。

#### `fork`

由於還沒有 virtual memory ，因此必須先算出 <ruby>user process 的 `sp`<rt> s s c r a t c h 寄存器</rt></ruby> 跟 stack top 的 offset 並且加在新分配的記憶體上面， kernel stack 也是一樣，並且把 `pt_regs` 存到 kernel stack 裡面。記得要設定 parent process 以及清空 child process ；以及最後，設定 `ra` 為 `fork_ret` ，內容基本上跟 `handle_interrupt` 組合語言函數的後半段差不多，不過要將 <ruby>`a0` 寄存器<rt> f o r k 返回值</rt></ruby>設為 0 。

#### `waitpid`

和 UART 的 `rx`、`tx` 差不多，宣告一個 wait queue ，並且將目前正在執行的 process 放入 queue 裡面並且等待喚醒。

#### `stop`

必須建立一個 live process 的結構用來儲存所有存活的 process 。搜尋到存活 process 裡面對應的 process 的狀態改成 `terminal` ，等此 process 再次被調度時就會觸發回收。 

## Video Player

[Exercise 5-2][Exercise] 有詳細的程式碼可以參考，內容主要是將圖片置中。不過要另外實做兩個 system call 。

### `usleep`

將自己的狀態設定為 waiting ，並且加入 timer interrupt 的 task 裡面，在 task 裡面，將自己的狀態改成 ready 並且加入 ready queue 裡面。

## POSIX Signal

作業小魔王，需要花一點時間除錯。不過如果熟悉之前的 user process 的 context switch 等，這一項的實做原理應該相對簡單許多。首先是在 thread control block 裡面加入 `sig` 欄位：

```rust
pub struct SigAct {
    pub sig_mask: u64,
    pub sig_handler_func: [usize; u64::BITS as usize],
    pub sig_stack: Option<Box<[u8]>>,
}
```

`sig_mask` 負責讓程式檢測是否有 signal 傳過來， `sig_handler_func` 儲存 user mode 的 function 所在位置，預設為 0 ， `sig_stack` 則是 signal 被觸發時，獨立分配的 stack 。處理 signal 的過程大概是這樣的：
1. 在 interrupt 結束之後會檢查目前的 process 是否有訊號進來。
2. 檢查是否有註冊 handler ，如果沒有則直接 `do_exit`
3. 如果有註則 handler ，則分配 stack 並且把目前的 context 寫入 stack 的最頂端。
4. 更改 `sepc`、`sscratch`、`ra` 讓程式能夠進入 signal handler 函數，並且在結束時呼叫 `sig_ret` system call
5. 在 `sigreturn` system call 裡面釋放 signal stack 並且恢復原本的 context 。

> 註：我的寫法理論上支援 nesty signal 不過作業似乎沒有要求這點就是了。

## 心得
原本以為這麼難的作業程式碼應該會暴增，不過實際寫完之後，好像還好。這一個作業最難的部分就是在於一開始要如何初始化 idle thread 並且讓 scheduler 和 context switch 能夠正確跑起來，當這兩者皆已經完成之後，後面會相對起來輕鬆不少。還有就是使用 `Mutex` 難免會因為程式沒寫好 deadlock ，除錯起來其實挺麻煩的；最後就是大魔王 stack overflow ， Rust 在 debug 模式下會先建立物件再 copy 是我真的沒有想到的（在用 `Box` 建立 stack 那裡）， No way 找那裡 stack overflow 找了一整天！但是總之目前的程式是能夠正常運行的，可喜可賀。

另外一件事是我那時候突然異想天開，想要搞懂 spin lock 是怎麼寫的，所以自己寫了一個，包含後面的 once 我也自己刻了一個。現在仔細想想真的有點沒有必要，因為需要自己寫很多 method ，並且搞定 borrow 跟所有權的問題（就是在說你 `MutexGuard<T, '_>`），應該要等到學期結束再來搞的，不然現在裡面寫的這個 spin lock 其實就是個半成品，然後也有點不好用。我後來又改成使用 `spin::Mutex` 了，不過有些地方沒有改過來，程式碼變得有點混亂真是有點得不償失。
