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

  .bss : ALIGN(__ALIGN_SIZE) {
    *(.bss .bss.*)
  }

  .got : ALIGN(__ALIGN_SIZE) {
    *(.got .got.*)
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
