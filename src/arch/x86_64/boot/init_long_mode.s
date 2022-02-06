/*
 * Init code to jump the rust code
 */

.code64

.global init_long_mode
.extern multiboot_main, directboot_main, unknown_boot_main
.extern main_code_segment_descriptor, user_code_segment_descriptor, tss_descriptor, user_data_segment_descriptor
.extern BOOT_FROM_MULTIBOOT_MARK, BOOT_FROM_DIRECTBOOT_MARK
.extern OS_STACK_SIZE, os_stack, gdtr_64bit_1

.section .text.32

init_long_mode:
  /* Set segment registers to zero (DO NOT SET CS REGISTER) */
  xor   %ax, %ax
  mov   %ax, %es
  mov   %ax, %ss
  mov   %ax, %ds
  mov   %ax, %fs
  mov   %ax, %gs

  lgdt  gdtr_64bit_1
  /* Write TSS segment information */
  movabs    $tss, %rax
  movabs    $tss_descriptor_address, %rbp
  mov   %ax, 2(%rbp)
  shr   $16, %rax
  mov   %al, 4(%rbp)
  mov   %ah, 7(%rbp)
  shr   $16, %rax
  mov   %eax, 8(%rbp)
  /* Set 64bit TSS */
  mov   $tss_descriptor, %ax
  ltr   %ax

  pop   %rdi                                /* Pass boot information */
  mov   $main_code_segment_descriptor, %rsi
  mov   $user_code_segment_descriptor | 3, %rdx
  mov   $user_data_segment_descriptor | 3, %rcx
  pop   %rax                                /* Boot type (Multiboot:1, Directboot: 2)*/
  movabs    $(os_stack + OS_STACK_SIZE), %rsp   /* Reset Stack */
  cmp   $BOOT_FROM_MULTIBOOT_MARK, %rax
  jz    jump_to_multiboot_main
  cmp   $BOOT_FROM_DIRECTBOOT_MARK, %rax
  jz    jump_to_directboot_main
  movabs    $unknown_boot_main, %rax             /* at src/arch/x86_64/mod.rs */
  jmp   *%rax

jump_to_multiboot_main:
  movabs    $multiboot_main, %rax                /* at src/arch/x86_64/mod.rs */
  jmp       *%rax

jump_to_directboot_main:
  movabs    $directboot_main, %rax               /* at src/arch/x86_64/mod.rs */
  jmp       *%rax
