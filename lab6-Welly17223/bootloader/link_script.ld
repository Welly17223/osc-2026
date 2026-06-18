MEMORY {
 /* program (rwx) : ORIGIN = 0x80200000, LENGTH = 2 * 1024 * 1024 */
    program (rwx) : ORIGIN = 0x100000000, LENGTH = 2 * 1024 * 1024
}

SECTIONS {
    .text.boot : {
        *(.text.boot)
  } > program

  .text : {
      *(.text)
  } > program

  .data : {
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
  . = . + 0x4000;
  . = ALIGN(16);
  _end = .;
}
