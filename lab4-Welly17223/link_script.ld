MEMORY {
    program (rwx) : ORIGIN = 0x20000000, LENGTH = 8 * 1024 * 1024
    /* program (rwx) : ORIGIN = 0x80200000, LENGTH = 8 * 1024 * 1024 */
}

SECTIONS {
    .text.boot : {
        *(.text.boot)
  } > program

  .text : {
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
  } > program

  . = ALIGN(16);
  __stack_buttom = .;

  . = . + 0x100000;
  . = ALIGN(16);
  __stack_top = .;
}
