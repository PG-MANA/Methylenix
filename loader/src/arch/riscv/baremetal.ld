OUTPUT_FORMAT(binary)
/* OUTPUT_FORMAT(elf64-littleriscv) */
ENTRY(_start)

SECTIONS {
  /* This is the magic number, please see `_start` also */
  . = 0xAA550000;

  .start : {
    *(.start)
  }

  . = ALIGN(8);
  __REL_START = .;

  .data : {
    KEEP(*(.data.rel*))
  }

  . = ALIGN(8);
  .got : {
    KEEP(*(.got))
  }
  __REL_END = .;
  
  .text : {
    *(.text .text.*)
  }

  .data : {
    *(.data)
  }
  
  .rodata : {
    *(.rodata .rodata.*)
  }

  __BSS_START = .;
  .bss : {
    KEEP(*(.bss .bss.*))
  }
  . = ALIGN(8);
  __BSS_END = .;

  .kernel : {
    *(.kernel)
  }

  __LOADER_END = .;

  /DISCARD/ : {
    *(.comment .comment.*)
    *(.eh_frame .eh_frame.*)
    *(.eh_frame_hdr .eh_frame_hdr.*)
  }
}
