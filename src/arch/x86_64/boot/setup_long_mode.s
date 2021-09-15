/*
 * Setup code to enter x86_64 mode
 */

.code32

.global setup_long_mode, fin
.extern init_long_mode
.extern main_code_segment_descriptor, tss_descriptor, gdtr_64bit_0 /* at common.asm */
.extern tss_descriptor_address, tss, pd, pdpt, pml4
.extern __KERNEL_MAP_START_ADDRESS

.section .text.32

setup_long_mode:
  /* Check if cpu supports x86_64 */
  /* https://wiki.osdev.org/setting_up_long_mode#detection_of_cpuid */
  mov  $0xff, %al
  out  %al, $0x21
  nop
  out  %al, $0xa1
  cli

  pushfd
  pop   %eax
  mov   %eax, %ecx
  xor   $(1 << 21), %eax        /* Xor ID flag and push to check if extra cpuid is supported */
  push  %eax
  popfd
  pushfd
  pop   %eax
  push  %ecx
  popfd                         /* Restore original eflags */
  xor   %ecx, %eax
  jz    cpuid_not_supported
  mov   $0x80000000, %eax       /* Is extra cpuid supported? */
  cpuid
  cmp   $0x80000001, %eax
  jb    only_x86
  mov   $0x80000001, %eax
  cpuid
  test  $(1 << 29), %edx        /* Long Mode Enable Bit */
  /* (AMD64 Architecture Programmer’s Manual, Volume 2: System Programming - 14.8 Long-Mode Initialization Example) */
  jz    only_x86

  /* Paging */
  /* 2-MByte Paging */
  /* PML4->PDP->PD */
  /* Direct map 0 ~ 4GiB */
  xor   %ecx,  %ecx
pde_setup:
  mov   $0x200000, %eax         /* eax = 2MB(direct mapping) */
  mul   %ecx                    /* eax = eax * ecx(0..2048) */
  or    $0b10000011, %eax       /* Present + R/W + Huge */
  mov   %eax, pd(, %ecx, 8)      /* pd[ecx * 8] = eax (higher 32bit is zero) */
  inc   %ecx
  cmp   $2048, %ecx
  jne   pde_setup               /* ecx != 512 * 4 */

  xor   %ecx, %ecx
pdpte_setup:
  mov   $4096, %eax             /* eax = 4KiB(size of page directory) */
  mul   %ecx
  add   $pd, %eax               /* eax = 4096 * ecx + pd(edx) */
  or    $0b11, %eax             /* Present + R/W */
  mov   %eax, pdpt(, %ecx, 8)    /* pdpt[ecx * 8] = eax (higher 32bit is zero) */
  inc   %ecx
  cmp   $4, %ecx
  jne   pdpte_setup

/* pml4_setup: */
  mov   $pdpt, %eax
  or    $0b11, %eax             /* Present + R/W */
  mov   %eax, (pml4)
/* Map also KERNEL_MAP_START_ADDRESS */
  mov   $((KERNEL_MAP_START_ADDRESS >> 39) & 0x1FF), %edx
  mov   %eax, pml4(, %edx, 8)

/* setup_64: */
  mov   $pml4, %eax
  mov   %eax, %cr3
  mov   %cr4, %eax
  or    $(1 << 5), %eax
  mov   %eax, %cr4              /* Set PAE flag */
  mov   $0xc0000080, %ecx
  rdmsr                         /* Model-specific register */
  or    $(1 << 8 | 1 << 11), %eax
  wrmsr                         /* Set LME and NXE flags */
  mov   %cr0, %eax
  or    $(1 << 31 | 1), %eax    /* Set PG flag */
  lgdt  gdtr_64bit_0
  mov   %eax, %cr0
  ljmp $main_code_segment_descriptor, $init_long_mode


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

.section .data.32

.align   4

long_mode_error_str:
  /* Attention: little endian */
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
