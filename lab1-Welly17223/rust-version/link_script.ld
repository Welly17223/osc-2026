MEMORY {
    program (rwx) : ORIGIN = 0x80200000, LENGTH = 2 * 1024 * 1024
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
      __bss_start = .;
      *(.bss)
      __bss_stop = .;
  } > program

  . = ALIGN(16);
  . = . + 4096;
  _end = .;
}
