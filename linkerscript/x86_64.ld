
SECTIONS {
    ENTRY(boot_entry) /*起動処理のコード*/
    . = 1M;
    LD_KERNEL_START = .;
    
    .boot_entry BLOCK(0x1000): ALIGN(0x1000) { /*BLOCKとALIGNは同意義*/
        KEEP(*(.grub_header))/*使われてないとリンクされなくなるので*/
    }

    .text BLOCK(4K): ALIGN(4K) {
        *(.text*)
    }

    .data BLOCK(4K): ALIGN(4K) {
        *(.data*)
        *(.rodata*)
    }

    .rodata BLOCK(4K): ALIGN(4K) {
        *(.rodata*)
    }

    .bss BLOCK(4K): ALIGN(4K) {
        *(.bss*)
    }

    .data.rel.ro BLOCK(4K): ALIGN(4K) { /*動的再配置?*/
        *(.data.rel.ro.local*)
        *(.data.rel.ro*)
    }
}
 
