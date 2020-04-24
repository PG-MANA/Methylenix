OUTPUT_FORMAT(elf64-x86-64)
ENTRY(boot_entry)

__ALIGN_SIZE = 0x1000;

SECTIONS {

  . = 0x100000 + SIZEOF_HEADERS;

  .boot_entry : ALIGN(8) {
    KEEP(*(.multiboot_header))/*使われてないとリンクされなくなるので*/
  }

  .text : ALIGN(__ALIGN_SIZE) {
    *(.text .text.*)
  }

  .data : ALIGN(__ALIGN_SIZE) {
    *(.data .data.*)
  }

  .rodata : ALIGN(__ALIGN_SIZE) {
    *(.rodata .rodata.*)
  }

  .bss : ALIGN(__ALIGN_SIZE) {
    *(.bss .bss.*)
  }

  .got : ALIGN(__ALIGN_SIZE) {
    *(.got .got.*)
  }
}
