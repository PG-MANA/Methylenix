/*
 * Init code to jump the rust code
 */

.code64
.att_syntax

.global init_x86_64
.extern boot_main
.extern main_code_segment_descriptor, user_code_segment_descriptor, user_data_segment_descriptor
.extern BOOT_FROM_MULTIBOOT_MARK, BOOT_FROM_DIRECTBOOT_MARK
.extern OS_STACK_SIZE, os_stack

.section .text

init_x86_64:
  /* Set segment registers to zero (DO NOT SET CS REGISTER) */
  xor   %ax, %ax
  mov   %ax, %es
  mov   %ax, %ss
  mov   %ax, %ds
  mov   %ax, %fs
  mov   %ax, %gs
  pop   %rdi                /* Pass bootinformation */
  mov   $main_code_segment_descriptor, %rsi
  mov   $user_code_segment_descriptor, %rdx
  mov   $user_data_segment_descriptor, %rcx
  pop   %rax                /* Boot type (Multiboot:1, Directboot: 2)*/
  mov   $(os_stack + OS_STACK_SIZE), %rsp
  cmp   $BOOT_FROM_MULTIBOOT_MARK, %rax
  jz    multiboot_main      /* at src/arch/x86_64/mod.rs */
  cmp   $BOOT_FROM_DIRECTBOOT_MARK,%rax
  jz    directboot_main     /* at src/arch/x86_64/mod.rs */
  jmp   unknownboot_main    /* at src/arch/x86_64/mod.rs */
