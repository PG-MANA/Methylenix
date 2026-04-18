/*
 * Init code to jump the rust code
 */

.code64

.global   init_long_mode
.extern   multiboot_main, boot_main
.extern   main_code_segment_descriptor, user_code_segment_descriptor, tss_descriptor, user_data_segment_descriptor
.extern   BOOT_FROM_MULTIBOOT_MARK, BOOT_FROM_DIRECTBOOT_MARK
.extern   OS_STACK_SIZE, os_stack, gdtr_64bit_1

.section  .text
.type     init_long_mode, %function
init_long_mode:
  /* Set segment registers to zero (DO NOT SET CS REGISTER) */
  xor     %ax, %ax
  mov     %ax, %es
  mov     %ax, %ss
  mov     %ax, %ds
  mov     %ax, %fs
  mov     %ax, %gs

  lea     gdtr_64bit_1(%rip), %rax
  lgdt    (%rax)

  /* Write TSS segment information */
  lea     tss(%rip), %rbx
  lea     tss_descriptor_address(%rip), %rbp
  mov     %bx, 2(%rbp)
  shr     $16, %rbx
  mov     %bl, 4(%rbp)
  mov     %bh, 7(%rbp)
  shr     $16, %rbx
  mov     %ebx, 8(%rbp)
  mov     $tss_descriptor, %ax
  ltr     %ax                                   /* Set 64bit TSS */

  pop     %rdi                                  /* Pass boot information */
  mov     $main_code_segment_descriptor, %rsi
  mov     $user_code_segment_descriptor | 3, %rdx
  mov     $user_data_segment_descriptor | 3, %rcx
  pop     %rax                                  /* Boot type (Multiboot:1, Loader: 2) */

  cmp     $BOOT_FROM_MULTIBOOT_MARK, %rax
  jz      jump_to_multiboot_main
2:
  hlt
  jmp     2b
.size     init_long_mode, . - init_long_mode

.type     jump_to_multiboot_main, %function
jump_to_multiboot_main:
  lea     (os_stack + OS_STACK_SIZE)(%rip), %rsp
  jmp     multiboot_main                        /* at src/arch/x86_64/mod.rs */
.size     jump_to_multiboot_main, . - jump_to_multiboot_main

