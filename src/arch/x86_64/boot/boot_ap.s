/*
 * boot entry code for application processors
 */


.att_syntax

.global ap_entry, ap_entry_end, ap_os_stack_address

.extern main_code_segment_descriptor, tss_descriptor, gdtr0 /* at common.asm */
.extern tss_descriptor_adress, tss, pml4
.extern ap_boot_main

.section .data

/* (ap_entry ~ ap_entry_end) will be copied */
.code16
ap_entry:
    cli
    /* CS seems set page number provided by SIPI */
    mov     %cs, %ax
    mov     %ax, %ds
    mov     %ax, %ss
    xor     %ebx, %ebx /* EBX = 0 */
    mov     %bx, %ax
    shl     $12, %ebx /* EBX is base address */
    /* Enable A20M# pin */
    /* Intel 64 and IA-32 Architectures Software Developer’s Manual Volume 3
       8.7.13.4 External Signal Compatibility
       Newer Intel 64 processor may not have A20M# pin.
    */
    mov     $0, %ax
    in      $0x92, %al
    or      $0x02, %al
    out     %al, $0x92

    /* Set jump address into ljmpl */
    mov    $(ljmpl_32_address - ap_entry), %eax
    add     %ebx, (%ebx, %eax) /* add base */

    /* Set GDT address into gdtr */
    mov     $(gdtr_32bit - ap_entry), %eax
    add     %ebx, 2(%ebx, %eax)

    add     %ebx, %eax
    lgdt    (%eax)

    mov     %cr0, %eax
    and     $0x7fffffff, %eax /* disable paging */
    or      $0x00000001, %eax /* enable 32bit protect mode */
    mov     %eax, %cr0

    /* Long JMP */
    .byte 0x66, 0xea                        /* opcode and 32bit address prefix(?) */
ljmpl_32_address:
    .long (ap_init_long_mode - ap_entry)    /* offset (base will be added before) */
    .word gdt_32bit_code_segment_descriptor /* code segment */


.code32
ap_init_long_mode:
    mov     $gdt_32bit_data_segment_descriptor, %ax
    mov     %ax, %ds

    /* Set jump address into ljmpl */
    mov    $(ljmpl_64_address - ap_entry), %eax
    add     %ebx, (%ebx, %eax) /* add base address */

    mov     $pml4, %eax
    mov     %eax, %cr3
    mov     %cr4, %eax
    or      $(1 << 5), %eax
    mov     %eax, %cr4                  /* Set PAE flag */
    mov     $0xc0000080, %ecx
    rdmsr                               /* Model-specific register */
    or      $(1 << 8 | 1 << 11), %eax
    wrmsr                               /* Set LME and NXE flags */
    mov     %cr0, %eax
    or      $(1 << 31 | 1), %eax        /* Set PG flag */
    lgdt    gdtr0
    mov     %eax, %cr0

    /* Long JMP */
    .byte   0xea                        /* opcode */
ljmpl_64_address:
    .long (ap_init_x86_64 - ap_entry)   /* offset (base will be added before) */
    .word main_code_segment_descriptor  /* code segment */


.code64
ap_init_x86_64:
    xor     %ax, %ax
    mov     %ax, %es
    mov     %ax, %ss
    mov     %ax, %ds
    mov     %ax, %fs
    mov     %ax, %gs
    /* TODO: Set 64bit TSS */
    /* Set stack */
    mov     $(ap_os_stack_address - ap_entry), %eax
    add     %ebx, %eax      /* EBX has base address */
    mov     (%eax), %rsp
    lea     ap_boot_main, %rax
    jmp    *%rax            /* "*" means absolute jmp */


.align  16

gdt_32bit:
    /* NULL DESCRIPTOR */
    .quad   0
.equ gdt_32bit_code_segment_descriptor, . - gdt_32bit
    /* Code */
    .word   0xffff, 0x0000, 0x9b00, 0x00cf
.equ gdt_32bit_data_segment_descriptor, . - gdt_32bit
    /* R/W */
    .word   0xffff, 0x0000, 0x9200, 0x00cf
    .word   0
gdtr_32bit:
    .word  . - gdt_32bit - 1
    .long  gdt_32bit - ap_entry

.align 8

ap_os_stack_address:
    .quad   0

ap_entry_end:
