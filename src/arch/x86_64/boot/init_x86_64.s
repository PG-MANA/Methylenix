/*
 * init cpu to jump rust code
 */

.code64
.att_syntax

.global init_x86_64
.extern boot_main
.extern main_code_segment_descriptor, user_code_segment_descriptor, user_data_segment_descriptor

.section .text

init_x86_64:
  /* init segment registers (do not init CS register) */
  xor   %rax,  %rax
  mov   %ax, %es
  mov   %ax, %ss
  mov   %ax, %ds
  mov   %ax, %fs
  mov   %ax, %gs
  pop   %rdi                /* pass multibootinformation */
  mov   $main_code_segment_descriptor, %rsi
  mov   $user_code_segment_descriptor, %rdx
  mov   $user_data_segment_descriptor, %rcx
  mov   $(stack + STACK_SIZE), %rsp
  jmp   boot_main           /* at src/arch/x86_64/mod.rs */

.section .bss

.align   4

.equ STACK_SIZE, 0x1000
.comm stack, STACK_SIZE
