/* QEMU-virt-like layout: 1 MiB of RAM at 0x80000000, stack at its top. */
ENTRY(_start)

MEMORY
{
    RAM (rwx) : ORIGIN = 0x80000000, LENGTH = 1M
}

SECTIONS
{
    .text : { *(.text.start) *(.text .text.*) } > RAM
    .rodata : { *(.rodata .rodata.* .srodata .srodata.*) } > RAM
    .data : { *(.data .data.* .sdata .sdata.*) } > RAM
    .bss (NOLOAD) : ALIGN(4) {
        __bss_start = .;
        *(.bss .bss.* .sbss .sbss.* COMMON)
        . = ALIGN(4);
        __bss_end = .;
    } > RAM
    __stack_top = ORIGIN(RAM) + LENGTH(RAM);
}
