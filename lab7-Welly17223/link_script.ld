OUTPUT_ARCH(riscv)
ENTRY(_start)
MEMORY {
    /* program (rwx) : ORIGIN = 0x20000000, LENGTH = 8 * 1024 * 1024 */
    /* program (rwx) : ORIGIN = 0x80200000, LENGTH = 8 * 1024 * 1024 */
    program (rwx) : ORIGIN = 0xffffffc000200000, LENGTH = 2M
}

SECTIONS {
  __base = 0xffffffc000000000;

  .text : {
      *(.text.boot)
      *(.text)
  } > program

  .data : {
      __global_pointer = .;
      *(.data)
  } > program

  .rodata : {
      *(.rodata)
  } > program

  .bss : {
      . = ALIGN(8);
      __bss_start = .;
      *(.bss)
      . = ALIGN(8);
      __bss_stop = .;

      . = ALIGN(16);
      __stack_buttom = .;
  } > program


  __stack_top = ORIGIN(program) + LENGTH(program);
  __kernel_start = ORIGIN(program);
  __kernel_end = ORIGIN(program) + LENGTH(program);
}
