# 2026 陽明交通大學資工系 Operator System Capstone 課程簡介

## 相關網站
課程網站：[OSC](https://people.cs.nycu.edu.tw/~ttyeh/course/2026_Spring/IOC5226/outline.html)

作業網站：[作業](https://nycu-caslab.github.io/OSC2026/class/staff.html)

## 系統架構
Risc-V。課堂會提供 Orange PI RV 2 讓學生使用。

## 加簽
這一屆由於板子不太夠，因此開學之後考了一個開學考，取最高分的前 55 人。建議最好提前 10\~20 分鐘進教室 ~有搶到座位贏一半~，否則晚到的就只能趴在地上寫券了。

## 分數計算

| 項目 | 比例 |
| -------------- | --------------- |
| Lab | 95 % |
| 出席率 | 5 % |

## 作業簡介

我們這一屆可以使用任何語言，你可以用 asm 也可以用 python，只要作業能寫出來、Demo 不會炸掉就好。筆者用 Rust，也有看到用 Zig 寫的；但是非 C 語言的程式碼一樣要可以跑助教的測資（主要是 Lab 3） 所以要提供 C 的 ABI。

- **Lab0**: 開機，並且讓助教寫得 kernel 跑起來，不算分。
- **Lab1**: 讓自己的 kernel 跑起來、Uart 印出東西、寫 OpenSBI 的 API
- **Lab2**:
  - **Uart boot loader**: 用 Uart 接收傳進去的資料並且執行
  - **Ram Disk、Device Tree**: 寫這兩個東西的 Resolver。
- **Lab3**: Memory Allocator。困難作業的開始，這個作業之後所有作業的完成時間都是以「週」來計算。
- **Lab4**: Exception & Interrupt。啟動並且完成以下 Interrupt:
  - **Timer**: 設定 timer 中斷，Advance 要管理多重 timer 中斷。
  - **EnvironmentCallFromUmode**: 偵測 System Call。並且要讓簡單的使用者程式（儲存在前面寫好的 Ram Disk 裡面）能夠跑起來。
  - **External**: 設定 PLIC，讓 external interrupt 能夠被 hart 0 執行。並且讓 Uart 的讀取以及寫入變成 Async 的。
  - **Nested Interrupt**: 在 Interrupt 裡面執行其他 interrupt，並且在執行完成之後能夠回去執行前面的 Interrupt。
- **Lab5**: 第二難的作業，寫 User process 的 Context switch、排程、System Call。
  - **System Call 包含**： getpid、read uart、write uart、exec、fork、waitpid、exit、stop 等 
  - **Video player**：稍微簡單的東西，主要透過播放影片讓助教確認同時有多個程式在執行。實做 System call: USleep、Display
  - **POSIX Signal**: 讓 user process 可以註冊 signal handler 以及送 signal
- **Lab6**: 最難作業，有三個禮拜寫。Virtual Memory。把 kernel 搬到 virtual memory 的執行空間，並且做權限設定。也會順便實做包含 CoW 等 fork 的機制。
- **Lab7**: Virtual File System。讓 user process 能夠 chdir、mkdir、create、open、read、write 檔案（System Call），並且要支援以下檔案系統：
  - **Ram Disk**: Ram Disk 的含金量還在提升，唯讀。
  - **Tmpfs**: 一個暫時的檔案系統，將資料全部存在記憶體裡面，主要為了驗證 virtual file system 的功能是否完整。

個人難度排序：6 > 5 > 3 > 4 >= 7 > 2 > 1

Lab 7 的程式碼總量
```sh
❯ wc `rg --files --type=rust --type=c --type=asm ./oslib ./src ./c_test` -l | sort -n
     1 ./oslib/src/interrupt/input_handler.rs
     3 ./c_test/src/main.rs
    11 ./c_test/build.rs
    15 ./oslib/src/platform.rs
    22 ./oslib/build.rs
    28 ./oslib/src/sbi.c
    32 ./oslib/src/logger.rs
    34 ./oslib/src/lib.rs
    48 ./oslib/src/file_system/byte_device.rs
    50 ./oslib/src/once.rs
    60 ./oslib/src/memory_alloc/startup_alloc.rs
    65 ./src/start.s
    70 ./oslib/src/context_switch.S
    77 ./oslib/src/file_system/file_describtor_table.rs
    81 ./oslib/src/spinlock.rs
    95 ./c_test/src/lib.rs
    99 ./oslib/src/interrupt/plic.rs
   102 ./oslib/src/sbi.rs
   105 ./oslib/src/interrupt/handle_exception.S
   142 ./oslib/src/display.rs
   143 ./src/main.rs
   153 ./oslib/src/interrupt/page_fault.rs
   164 ./oslib/src/ramdisk.rs
   183 ./oslib/src/file_system/ramdisk.rs
   198 ./c_test/src/test_memory.c
   229 ./oslib/src/interrupt/timer.rs
   282 ./oslib/src/file_system/tempfs.rs
   367 ./oslib/src/fdt.rs
   396 ./oslib/src/kernel_shell.rs
   431 ./oslib/src/schedule.rs
   453 ./oslib/src/memory_alloc/dynamic_alloc.rs
   457 ./oslib/src/memory_alloc/mod.rs
   479 ./oslib/src/thread.rs
   546 ./oslib/src/virtual_mem/mod.rs
   572 ./oslib/src/file_system/mod.rs
   592 ./oslib/src/interrupt/u_mode.rs
   626 ./oslib/src/uart.rs
   674 ./oslib/src/interrupt/mod.rs
   734 ./oslib/src/memory_alloc/buddy_alloc.rs
  8819 total
```

## Demo
除了要能夠跑完助教的測試流程驗證解果的正確性以外，助教也會問**很多**跟 Lab 內容或是觀念有關的問題；導致整個 Demo 的時間會拉得很長，大概 30 分鐘或是更久。可以補 Demo，一週扣 10% 算是滿佛的，有寫作業有寫 Advance 都可以超過 100 分，所以補 Demo 分數也不會太差。

## 除錯小撇步
有寫奇怪的錯誤是平常寫程式不會發生但是上 kernel 就遇到的，下面提供幾個在所有 Lab 有遇到的跟怪怪情況，各自作業的詳細寫作歷程會放在每個 Lab 資料夾裡面的 README.md 裡面：

1. Stack Overflow。如果記憶體給得不夠，有可能 stack 寫到 text 或是 data，建議可以確認一下 sp 的值是否正常。
2. Used After Free 等記憶體問題。這次沒有作業系統幫你處理這些，你需要發揮超常的注意力來檢查。
3. GDB 是你的好幫手，搭配 Qemu 除錯是完成作業的關鍵之一。
4. 有沒有確認 I cache flush 了？如果有更新記憶體裡面的 instrunction，記得要 `fence.i`
5. Race Conditioin，後面比較會遇到，記得原子操作。
6. 記憶體寫入無效：如果寫入的記憶體是一個常數或者沒有特別標示，有可能會被編譯器當成無效寫入進而被刪除。有 Uart 等硬體記得 read/write volatile

# 修課心得
目前在交大修過最硬的課程，連公認很困難的 NA、SA 等在這堂課面前都必須退讓三分。教授上課主要會複習作業系統概論教過的內容，並且補充 Risc-V 上特有的硬體知識，像是有哪些特殊的 register 或是開機有哪些程序等；在寫作業之前會有不算分的 Excercise，能夠讓你在開始寫 Lab 之前有一些 Template Code，跟大致基本的概念。看到 Lab 的佔比就知道，修這門課主要就是來寫 Lab 的，也是在這門課我真正感受到「作業給你兩週是因為需要兩週才能寫完」，然後前面的 Lab 程式最好是夠毆穩定，不要在後面的 Lab 有東西壞掉了才發現前面有 Bug，搞不好你已經忘記前面是怎麽寫的了。由於 Lab 沒有限定什麼語言，因此最好選一個熟悉的，或是助教 Excercise 提供的 C 語言；如果你之前完全沒有寫過 Rust，那在使用前最好三思，畢竟 Rust 裡面有很多需要熟悉的概念，有可能你連編譯都過不了，不過 Rust 自帶記憶體安全的設計、外部依賴以及一些資料結構（像是 Link List、B Tree Map、String）在寫完 Memory Allocator 解禁之後，確實能夠一定程度上降低開發的壓力，不過也要你的 Memory Allocator 足夠穩定不出問題就是了。寫作業的過程需要常常查看各種 Risc-V 的 Spec，包含這些功能的運作流程以及需要使用的特殊 CSR，Device Tree、Ram Disk 這兩個作業基本上也是看著 Spec 去抓取資料的。

修完這門課程之後，能夠實做在作業系統概論裡面帶過的概念，並且實做在給定的硬體上。在這個過程中，你會對於編譯 bare matel 程式、開機流程更加了解，自己掌控每一個 byte 的記憶體分配跟釋放，並且理解記憶體、程式執行、資料的本質，將過去在計算機組織、作業系統概論、計算機概論與程式設計等課程學到的觀念整合，以及窺探 Linux 這個偉大的作業系統的設計之巧妙（作業不會寫就去參考相關的程式碼、程式分析文章）。當然，在歷經了這麼多時間的作業，你的作業系統仍然是不完整的，沒有多核心處理、沒有 Elf parser、不能上網、沒有原生程式......，這堂課僅僅只探討了作業系統最基礎、最核心的一部分。如果想要寫出一個「正常」的作業系統，前方仍是漫漫長路。
