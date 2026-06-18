.section ".text.boot"
.global _start
_start:
    la a3, __bss_start
    la a4, __bss_stop
    ble a4, a3, clear_bss_done
clear_bss:
    sd zero, (a3)
    add a3, a3, 8
    blt a3, a4, clear_bss
clear_bss_done:
    # a1 is the dtb address, pass it to the main loop
    la sp, _end
    jal relocation

.section ".text"
.global _start_main
_start_main:
    la a3, __bss_start
    la a4, __bss_stop
    ble a4, a3, clear_bss_main
clear_bss_main:
    sd zero, (a3)
    add a3, a3, 8
    blt a3, a4, clear_bss_main
clear_bss_done_main:
    # a1 is the dtb address, pass it to the main loop
    la sp, _end
    jal main
shutdown:
    wfi
    j shutdown

# (address: u64, instruct: u64)
.section ".text"
.global write_address
write_address:
  sb a1, 0(a0)
  jr ra

.section ".text"
.global read_address
read_address:
  lb a0, 0(a0)
  jr ra

# (kernel_addr: c_ulong, heart_id: c_ulong, dtb_addr: c_ulong)
.section ".text"
.global jump_to_kernel
jump_to_kernel:
  # save ra
  addi sp, sp, -16
  sd ra, 0(sp)
  # a0 is address of target
  mv t0, a0
  # a0 is dtb_addr
  mv a0, a1
  # a1 is dtb_addr
  mv a1, a2
  # jump to target address
  fence.i
  jalr ra, t0, 0
  # restore ra address
  ld ra, 0(sp)
  addi sp, sp, 16
  # jump back to ra
  jr ra
