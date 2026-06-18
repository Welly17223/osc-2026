SECTIONS
{
    . = 0x80200000;
    .text : { *(.text) }
    .rodata : { *(.rodata) }
    .data : { *(.data) }
    .bss : { *(.sbss) *(.bss) }
}
