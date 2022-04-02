OUTPUT_FORMAT(elf64-aarch64)
ENTRY(_boot_main)

__ALIGN_SIZE = 0x1000;
__KERNEL_MAP_START_ADDRESS = 0xffffff8000000000;

SECTIONS {

  . = 0x100000 + SIZEOF_HEADERS;

  .note : ALIGN(4) {
    *(.note)
  }

  . += __KERNEL_MAP_START_ADDRESS;
  . = ALIGN(__ALIGN_SIZE);
  .text : AT(ADDR(.text) - __KERNEL_MAP_START_ADDRESS) {
    *(.text .text.*)
  }

  . = ALIGN(__ALIGN_SIZE);
  .data : AT(ADDR(.data) - __KERNEL_MAP_START_ADDRESS) {
    *(.data .data.*)
  }

  . = ALIGN(__ALIGN_SIZE);
  .rodata : AT(ADDR(.rodata) - __KERNEL_MAP_START_ADDRESS) {
    *(.rodata .rodata.*)
  }

  . = ALIGN(__ALIGN_SIZE);
  .bss : AT(ADDR(.bss) - __KERNEL_MAP_START_ADDRESS) {
    *(.bss .bss.*)
  }

  . = ALIGN(__ALIGN_SIZE);
  .got : AT(ADDR(.got) - __KERNEL_MAP_START_ADDRESS) {
    *(.got .got.*)
  }

  . = ALIGN(__ALIGN_SIZE);
  .eh_frame : AT(ADDR(.eh_frame) - __KERNEL_MAP_START_ADDRESS) {
    *(.eh_frame .eh_frame.*)
  }

  . = ALIGN(__ALIGN_SIZE);
  .eh_frame_hdr : AT(ADDR(.eh_frame_hdr) - __KERNEL_MAP_START_ADDRESS) {
    *(.eh_frame_hdr .eh_frame_hdr.*)
  }


  /DISCARD/ : {
    *(.comment .comment.*)
  }
}
