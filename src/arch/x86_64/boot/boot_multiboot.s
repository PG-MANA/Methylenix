/*
 * Boot entry for Multiboot
 */

.code32

.global boot_multiboot, BOOT_FROM_MULTIBOOT_MARK
.extern setup_long_mode, fin                /* at setup_long_mode.s */
.extern OS_STACK_SIZE, os_stack             /* at common.s */

.equ MULTIBOOT_CHECK_MAGIC, 0x36d76289      /* multiboot2 magic code */
.equ BOOT_FROM_MULTIBOOT_MARK, 1

.section .text
.align 4

boot_multiboot:
  mov   $(os_stack + OS_STACK_SIZE), %esp

  push  $0
  popfd                             /* Clear eflags */
  push  $0                          /* for 64bit pop */
  push  $BOOT_FROM_MULTIBOOT_MARK   /* the mark booted from multiboot */
  push  $0                          /* for 64bit pop */
  push  %ebx                        /* save multiboot information */

  cmp   $MULTIBOOT_CHECK_MAGIC, %eax
  jne   bad_magic
  jmp   setup_long_mode

bad_magic:
  mov   $BOOT_ERROR_STR_SIZE, %ecx
  mov   $0xb8000, %edi
  mov   $boot_error_str, %esi
  rep   movsb
  jmp   fin

.section .data

.align   4

boot_error_str:
  /* Attention: little endian */
  /* 0x4f: back-color:red, color:white */
  .byte  'E', 0x4f
  .byte  'r', 0x4f
  .byte  'r', 0x4f
  .byte  'o', 0x4f
  .byte  'r', 0x4f
  .byte  ':', 0x4f
  .byte  'M', 0x4f
  .byte  'u', 0x4f
  .byte  'l', 0x4f
  .byte  't', 0x4f
  .byte  'i', 0x4f
  .byte  'b', 0x4f
  .byte  'o', 0x4f
  .byte  'o', 0x4f
  .byte  't', 0x4f
  .byte  ' ', 0x4f
  .byte  'i', 0x4f
  .byte  's', 0x4f
  .byte  ' ', 0x4f
  .byte  'n', 0x4f
  .byte  'o', 0x4f
  .byte  't', 0x4f
  .byte  ' ', 0x4f
  .byte  's', 0x4f
  .byte  'u', 0x4f
  .byte  'p', 0x4f
  .byte  'p', 0x4f
  .byte  'o', 0x4f
  .byte  'r', 0x4f
  .byte  't', 0x4f
  .byte  'e', 0x4f
  .byte  'd', 0x4f
  .byte  '.', 0x4f

.equ BOOT_ERROR_STR_SIZE, . - boot_error_str
