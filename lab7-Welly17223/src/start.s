.section ".text.boot"
.global _start
_start:
    la sp, __stack_top
    // save hart, dtb addr to callee saved register
    mv s1, a0
    mv s2, a1
    la s3, __kernel_start
    la s4, __kernel_end
    // init_virtual_memory(dtb_addr: usize, kernel_start: usize, kernel_end: usize)
    mv a0, a1
    mv a1, s3
    mv a2, s4
    call init_virtual_memory
    // virtual memory offset
    li s5, 0xffffffc000000000
    la t0, 1f
    sub t0, t0, s3
    add t0, t0, s5
    li s6, 0x200000
    add t0, t0, s6
    jr t0
1:
    // not use la because we are already in virtual memory
    // the function drop_identity needs physical memory address
    // use the symbol address loaded above
    // drop_identity(kernel_start: usize, kernel_end: usize)
    mv a0, s3
    mv a1, s4
    call drop_identity
    la a3, __bss_start
    la a4, __bss_stop
    ble a4, a3, clear_bss_done
clear_bss:
    sd zero, (a3)
    addi a3, a3, 8
    blt a3, a4, clear_bss
clear_bss_done:
    la a4, __stack_top
    # .option pop
    mv t0, sp
    la sp, __stack_top
    addi sp, sp, -32
    sd ra, 0(sp)
    sd t0, 8(sp)
    sd s0, 16(sp)
    la s0, __stack_top
    // a1 is the dtb address, pass it to the main loop
    // restore hart, dtb addr saved at start point
    mv a0, s1
    mv a1, s2
    // change dtb addr to virtual memory
    add a1, a1, s5
    sub a1, a1, s3
    add a1, a1, s6
    call main
shutdown:
    wfi
    j shutdown
    ld ra, 0(sp)
    ld t0, 8(sp)
    ld t1, 16(sp)
    mv sp, t0
    mv s0, t1
    ret
