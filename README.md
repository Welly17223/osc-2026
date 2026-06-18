# 課程簡介

## 2026 陽明交通大學資工系 Operator System Capstone 相關網站
課程網站：[OSC](https://people.cs.nycu.edu.tw/~ttyeh/course/2026_Spring/IOC5226/outline.html)
作業網站：[作業](https://nycu-caslab.github.io/OSC2026/class/staff.html)

## 系統架構
RiscV。課堂會提供 Orange PI RV 2 讓學生使用。

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
  - **Video player**)：稍微簡單的東西，主要透過播放影片讓助教確認同時有多個程式在執行。實做 System call: USleep、Display
  - **POSIX Signal**: 讓 user process 可以註冊 signal handler 以及送 signal
- **Lab6**: 最難作業，有三個禮拜寫。Virtual Memory。把 kernel 搬到 virtual memory 的執行空間，並且做權限設定。也會順便實做包含 CoW 等 fork 的機制。
- **Lab7**: Virtual File System。讓 user process 能夠 chdir、mkdir、create、open、read、write 檔案（System Call），並且要支援以下檔案系統：
  - **Ram Disk**: Ram Disk 的含金量還在提升，唯讀。
  - **Tmpfs**: 一個暫時的檔案系統，將資料全部存在記憶體裡面，主要為了驗證 virtual file system 的功能是否完整。

## Demo
除了要能夠跑完助教的測試流程，助教也會問**很多**跟 Lab 內容或是觀念有關的問題；導致整個 Demo 的時間會拉得很長，大概 30 分鐘或是更久。可以補 Demo，一週扣 10% 算是滿佛的，有寫作業有寫 Advance 都可以超過 100 分，所以補 Demo 分數也不會太差。

## 除錯小撇步
有寫奇怪的錯誤是平常寫程式不會發生但是上 kernel 就遇到的，下面提供幾個在所有 Lab 有遇到的跟怪怪情況，各自作業的詳細寫作歷程會放在每個 Lab 資料夾裡面的 README.md 裡面：

1. Stack Overflow。如果記憶體給得不夠，有可能 stack 寫到 text 或是 data，建議可以確認一下 sp 的值是否正常。
2. Used After Free 等記憶體問題。這次沒有作業系統幫你處理這些，你需要發揮超常的注意力來檢查。
3. GDB 是你的好幫手，搭配 Qemu 除錯是完成作業的關鍵之一。
4. 有沒有確認 I cache flush 了？如果有更新記憶體裡面的 instrunction，記得要 `fence.i`
5. Race Conditioin，後面比較會遇到，記得原子操作。
6. 記憶體寫入無效：如果寫入的記憶體是一個常數或者沒有特別標示，有可能會被編譯器當成無效寫入進而被刪除。有 Uart 等硬體記得 read/write volatile
