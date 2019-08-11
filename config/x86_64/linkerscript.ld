OUTPUT_FORMAT(elf64-x86-64)
ENTRY(boot_entry)

__ALIGN_SIZE = 0x1000;

SECTIONS {

  . = 0x100000 + SIZEOF_HEADERS;

  .boot_entry : {
    KEEP(*(.multiboot_header))/*使われてないとリンクされなくなるので*/
  }

  . = ALIGN(__ALIGN_SIZE);

  .text : {
    *(.text .text.*)
  }

  . = ALIGN(__ALIGN_SIZE);

  .data : {
    *(.data .data.*)
  }

  . = ALIGN(__ALIGN_SIZE);

  .rodata : {
    *(.rodata .rodata.*)
  }

  . = ALIGN(__ALIGN_SIZE);

  .bss : {
    *(.bss .bss.*)
  }
}
