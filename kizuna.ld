ENTRY(_start)

SECTIONS
{
    /*
     * QEMU virt RAM begins at 0x40000000.
     * We leave the first 1 MiB alone because QEMU places
     * its device tree near the beginning of RAM.
     */
    . = 0x40100000;

    .text : ALIGN(16)
    {
        KEEP(*(.text.boot))
        *(.text .text.*)
    }

    .rodata : ALIGN(16)
    {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(16)
    {
        *(.data .data.*)
    }

    .bss (NOLOAD) : ALIGN(16)
    {
        __bss_start = .;
        *(.bss .bss.* COMMON)
        __bss_end = .;
    }

    .stack (NOLOAD) : ALIGN(16)
    {
        . += 64K;
        __stack_top = .;
    }

    /DISCARD/ :
    {
        *(.comment)
        *(.eh_frame*)
    }
}
