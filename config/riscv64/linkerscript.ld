OUTPUT_FORMAT(elf64-riscv64)
ENTRY(boot_main)

__ALIGN_SIZE = 0x1000;

SECTIONS {
  .rela : ALIGN(__ALIGN_SIZE) {
    *(.rela.dyn .rela.plt)
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

  .eh_frame : ALIGN(__ALIGN_SIZE) {
    *(.eh_frame .eh_frame.*)
  }

  .eh_frame_hdr : ALIGN(__ALIGN_SIZE) {
    *(.eh_frame_hdr .eh_frame_hdr.*)
  }

  /DISCARD/ : {
    *(.comment .comment.*)
  }
}
