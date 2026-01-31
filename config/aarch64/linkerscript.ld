OUTPUT_FORMAT(elf64-aarch64)
ENTRY(boot_main)

__ALIGN_SIZE = 0x1000;
__KERNEL_MAP_START_ADDRESS = 0xffffff8000000000;

SECTIONS {
  . += __KERNEL_MAP_START_ADDRESS;

  .text : ALIGN(__ALIGN_SIZE) {
    *(.text .text.*)
  }

  .data : ALIGN(__ALIGN_SIZE) {
    *(.data .data.*)
  }

  .rodata : ALIGN(__ALIGN_SIZE) {
    *(.rodata .rodata.*)
  }

  .got : ALIGN(__ALIGN_SIZE) {
    *(.got.plt)
    *(.got)
    *(.igot)
    *(.igot.plt)
  }

  /*.rela : ALIGN(__ALIGN_SIZE) {
    *(.rela.dyn .rela.dyn.*)
    *(.rela*)
  }*/

  .bss : ALIGN(__ALIGN_SIZE) {
    *(.bss .bss.*)
  }

  /DISCARD/ : {
    *(.comment .comment.*)
    *(.eh_frame .eh_frame.*)
    *(.eh_frame_hdr .eh_frame_hdr.*)
  }
}
