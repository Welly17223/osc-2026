.section ".text.boot"
.global _start
_start:
    la a3, __bss_start
    la a4, __bss_stop
    ble a4, a3, clear_bss_done
clear_bss:
    sd zero, (a3)
    addi a3, a3, 8
    blt a3, a4, clear_bss
clear_bss_done:
    la a3, __stack_buttom
    la a4, __stack_top
    # .option push
    # .option norelax
    # la gp, __global_pointer
    # .option pop
    // a1 is the dtb address, pass it to the main loop
    mv t0, sp
    la sp, __stack_top
    addi sp, sp, -32
    sd ra, 0(sp)
    sd t0, 8(sp)
    sd s0, 16(sp)
    la s0, __stack_top
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
