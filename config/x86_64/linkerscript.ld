OUTPUT_FORMAT(elf64-x86-64)

__ALIGN_SIZE = 0x1000;

SECTIONS {

  ENTRY(boot_entry) /*起動処理のコード*/
  . = 1M; /*Memory Mapで空いていて使いやすそうな場所*/
  LD_KERNEL_START = .;

  .boot_entry : {
    KEEP(*(.multiboot_header))/*使われてないとリンクされなくなるので*/
  }

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
