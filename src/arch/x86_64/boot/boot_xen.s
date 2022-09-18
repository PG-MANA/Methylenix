/*
 * Boot code for Xen HVM Direct Boot
 */

.code32

.global     boot_xen, BOOT_FROM_DIRECTBOOT_MARK
.extern     setup_long_mode, fin                                    /* at init_long_mode.s */
.extern     OS_STACK_SIZE, os_stack, KERNEL_MAP_START_ADDRESS       /* at common.s */

.equ        XEN_START_INFO_MAGIC, 0x336ec578                        /* strat info magic code */
.equ        BOOT_FROM_DIRECTBOOT_MARK, 2

.section    .text.32
.align 4

.type       boot_xen, %function
boot_xen:
  mov   $(os_stack + OS_STACK_SIZE - KERNEL_MAP_START_ADDRESS), %esp

  push  $0
  popfd                             /* Clear eflags */
  push  $0                          /* for 64bit pop */
  push  $BOOT_FROM_DIRECTBOOT_MARK  /* the mark booted direct multiboot */
  push  $0                          /* for 64bit pop */
  push  %ebx                        /* save multiboot information */

  cmpl  $XEN_START_INFO_MAGIC, (%ebx)
  jne   xen_bad_magic
  jmp   setup_long_mode
.size   boot_xen, . - boot_xen

.type   xen_bad_magic, %function
xen_bad_magic:
  mov   $XEN_BOOT_ERROR_STR_SIZE, %ecx
  mov   $0xb8000, %edi
  mov   $xen_boot_error_str, %esi
  rep   movsb
  jmp   fin
.size   xen_bad_magic, . - xen_bad_magic

.section .data.32
.align   4

.type   xen_boot_error_str, %object
xen_boot_error_str:
  /* Attention: little endian */
  /* 0x4f: back-color:red, color:white */
  .byte  'E', 0x4f
  .byte  'r', 0x4f
  .byte  'r', 0x4f
  .byte  'o', 0x4f
  .byte  'r', 0x4f
  .byte  ':', 0x4f
  .byte  'B', 0x4f
  .byte  'a', 0x4f
  .byte  'd', 0x4f
  .byte  ' ', 0x4f
  .byte  'X', 0x4f
  .byte  'e', 0x4f
  .byte  'n', 0x4f
  .byte  ' ', 0x4f
  .byte  's', 0x4f
  .byte  't', 0x4f
  .byte  'a', 0x4f
  .byte  'r', 0x4f
  .byte  't', 0x4f
  .byte  ' ', 0x4f
  .byte  'i', 0x4f
  .byte  'n', 0x4f
  .byte  'f', 0x4f
  .byte  'o', 0x4f
  .byte  'r', 0x4f
  .byte  'm', 0x4f
  .byte  'a', 0x4f
  .byte  't', 0x4f
  .byte  'i', 0x4f
  .byte  'o', 0x4f
  .byte  'n', 0x4f
  .byte  ' ', 0x4f
  .byte  'm', 0x4f
  .byte  'a', 0x4f
  .byte  'g', 0x4f
  .byte  'i', 0x4f
  .byte  'c', 0x4f
  .byte  '.', 0x4f
.equ    XEN_BOOT_ERROR_STR_SIZE, . - xen_boot_error_str
.size   xen_boot_error_str, . - xen_boot_error_str
