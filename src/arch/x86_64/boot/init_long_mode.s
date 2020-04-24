/*
 * setup cpu to long mode
 */

.code32
.att_syntax

.global init_long_mode
.extern init_x86_64
.extern main_code_segment_descriptor, tss_descriptor, gdtr0 /* at common.asm */
.extern tss_descriptor_adress, tss, pd, pdpt, pml4

.section .text

init_long_mode:
  /* check if cpu supports x86_64 */
  /* http://wiki.osdev.org/setting_up_long_mode#detection_of_cpuid */
  mov  $0xff, %al
  out  %al, $0x21
  nop
  out  %al, $0xa1
  cli

  /* write TSS segment information */
  mov   $tss, %eax
  mov   $tss_descriptor_adress, %ebp
  mov   %ax, 2(%ebp)
  shr   $16, %eax
  mov   %al, 4(%ebp)
  mov   %ah, 7(%ebp)
  pushfd
  pop   %eax
  mov   %eax, %ecx
  xor   $(1 << 21), %eax        /* xor ID flag */
  push  %eax
  popfd
  pushfd
  pop   %eax
  push  %ecx
  popfd                         /* restore */
  xor   %ecx, %eax
  jz    cpuid_not_supported
  mov   $0x80000000, %eax       /* is extra cpuid supported? */
  cpuid
  cmp   $0x80000001, %eax
  jb    only_x86
  mov   $0x80000001, %eax
  cpuid
  test  $(1 << 29), %edx        /* Long Mode Enable Bit */
  /* (AMD64 Architecture Programmer’s Manual, Volume 2: System Programming - 14.8 Long-Mode Initialization Example) */
  jz    only_x86

  /* Paging */
  /* is 1GB paging supported? */
  test  $(1 << 26), %edx
  jz    init_normal_paging

init_4level_paging:
  xor   %ecx,  %ecx             /* counter */

pdpte_setup:
  /* Intel SDM chapter 4.5 LEVEL4-PAGING */
  /* 1-GByte Paging */
  /* PML4->PDP */

  mov   $0x100000, %eax         /* ページ一つが管理する区域(1GB) */
  mul   %ecx                    /* eax = eax * ecx */
  or    $0b10000011, %eax       /* Present + R/W + Huge(PDPTでHugeを立てると1GB単位) */
  mov   %eax, pdpt(,%ecx, 8)
  inc   %ecx
  cmp   $4, %ecx
  jne   pdpte_setup
  jmp   pml4_setup

init_normal_paging:
  xor   %ecx,  %ecx

pde_setup:
  /* 2-MByte Paging */
  /* PML4->PDP->PD */
  mov   $0x200000, %eax         /* ページ一つが管理する区域(2MB) */
  mul   %ecx                    /* eax = eax * ecx */
  or    $0b10000011, %eax       /* Present + R/W + Huge(PDPTでHugeを立てると1GB単位) */
  mov   %eax, pd(,%ecx, 8)      /* 64bitごとの配置 */
  inc   %ecx
  cmp   $2048, %ecx
  jne   pde_setup               /* ecx != 512 * 4 */

  xor   %ecx, %ecx

pdpte_setup_2mb:
  mov   $4096, %eax
  mul   %ecx
  add   $pd, %eax               /* eax = 4096 * ecx + pd(edx) */
  or    $0b11, %eax             /* Present + R/W */
  mov   %eax, pdpt(,%ecx, 8)
  inc   %ecx
  cmp   $4, %ecx
  jne   pdpte_setup_2mb

pml4_setup:
  mov   $pdpt, %eax
  or    $0b11, %eax             /* Present + R/W */
  mov   %eax, (pml4)

/* setup_64: */
  mov   $pml4, %eax
  mov   %eax, %cr3
  mov   %cr4, %eax
  or    $(1 << 5), %eax
  mov   %eax, %cr4              /* set PAE flag */
  mov   $0xc0000080, %ecx
  rdmsr                         /* model-specific register */
  or    $(1 << 8 | 1 << 11), %eax
  wrmsr                         /* set LME and NXE flags */
  mov   %cr0, %eax
  or    $(1 << 31 | 1), %eax    /* set PG flag */
  mov   $tss_descriptor, %dx
  lgdt  gdtr0
  mov   %eax, %cr0
  ltr   %dx
  ljmp $main_code_segment_descriptor, $init_x86_64


only_x86:
cpuid_not_supported:
  mov   $LONG_MODE_ERROR_STR_SIZE, %ecx
  mov   $0xb8000, %edi
  mov   $long_mode_error_str, %esi
  rep   movsb
fin:
  cli
  hlt
  jmp fin

.section .data

.align   4

long_mode_error_str:
  /* attention: little endian */
  /* 0x4f: back-color:red, color:white */
  .byte   'E',   0x4f
  .byte   'r',   0x4f
  .byte   'r',   0x4f
  .byte   'o',   0x4f
  .byte   'r',   0x4f
  .byte   ':',   0x4f
  .byte   '6',   0x4f
  .byte   '4',   0x4f
  .byte   'b',   0x4f
  .byte   'i',   0x4f
  .byte   't',   0x4f
  .byte   ' ',   0x4f
  .byte   'l',   0x4f
  .byte   'o',   0x4f
  .byte   'n',   0x4f
  .byte   'g',   0x4f
  .byte   ' ',   0x4f
  .byte   'm',   0x4f
  .byte   'o',   0x4f
  .byte   'd',   0x4f
  .byte   'e',   0x4f
  .byte   ' ',   0x4f
  .byte   'i',   0x4f
  .byte   's',   0x4f
  .byte   ' ',   0x4f
  .byte   'n',   0x4f
  .byte   'o',   0x4f
  .byte   't',   0x4f
  .byte   ' ',   0x4f
  .byte   's',   0x4f
  .byte   'u',   0x4f
  .byte   'p',   0x4f
  .byte   'p',   0x4f
  .byte   'o',   0x4f
  .byte   'r',   0x4f
  .byte   't',   0x4f
  .byte   'e',   0x4f
  .byte   'd',   0x4f
  .byte   '.',   0x4f

.equ LONG_MODE_ERROR_STR_SIZE, . - long_mode_error_str
