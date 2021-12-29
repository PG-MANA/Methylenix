/*
 * Boot entry for application processors
 */


.global ap_entry, ap_entry_end, ap_os_stack_address

.extern main_code_segment_descriptor, tss_descriptor, gdtr_64bit_0, gdtr_64bit_1 /* at common.asm */
.extern tss_descriptor_address, tss, pml4
.extern ap_boot_main

.section .data

/* (ap_entry ~ ap_entry_end) will be copied */
.code16
ap_entry:
    /* CS register has the offset to access the opcodes by "cs:ip(address = cs * 16 + ip)" */
    cli
    /* Calculate the relocation base address from CS */
    mov     %cs, %ax
    mov     %ax, %ds                            /* All data access uses ds:offset */
    xor     %ebx, %ebx                          /* EBX = 0 */
    mov     %ax, %bx
    shl     $4, %ebx                            /* EBX <<=4 ( EBX *= 16 ) */

    /* Set jump address into ljmpl */
    add     %ebx, ljmpl_32_address - ap_entry   /* add base address */

    /* Set GDT address into gdtr */
    add     %ebx, gdtr_32bit - ap_entry + 2

    lgdt    (gdtr_32bit - ap_entry)

    mov     %cr0, %eax
    and     $0x7fffffff, %eax                   /* disable paging */
    or      $0x00000001, %eax                   /* enable 32bit protect mode */
    mov     %eax, %cr0

    /* Long JMP */
    .byte 0x66, 0xea                            /* opcode and 32bit address prefix(?) */
ljmpl_32_address:
    .long (ap_setup_long_mode - ap_entry)       /* offset (base will be added before) */
    .word gdt_32bit_code_segment_descriptor     /* code segment */


.code32
ap_setup_long_mode:
    mov     $gdt_32bit_data_segment_descriptor, %ax
    mov     %ax, %ds

    /* Set jump address into ljmpl */
    mov    $(ljmpl_64_address - ap_entry), %eax
    add     %ebx, (%ebx, %eax)          /* add base address */

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
    lgdt    gdtr_64bit_0
    mov     %eax, %cr0

    /* Long JMP */
    .byte   0xea                        /* opcode */
ljmpl_64_address:
    .long (ap_init_long_mode - ap_entry)/* offset (base will be added before) */
    .word main_code_segment_descriptor  /* code segment */


.code64
ap_init_long_mode:
    xor     %ax, %ax
    mov     %ax, %es
    mov     %ax, %ss
    mov     %ax, %ds
    mov     %ax, %fs
    mov     %ax, %gs
    lgdt    gdtr_64bit_1
    /* Set stack */
    mov     $(ap_os_stack_address - ap_entry), %eax
    add     %ebx, %eax                  /* EBX has base address */
    mov     (%eax), %rsp
    movabs  $ap_boot_main, %rax
    jmp    *%rax                        /* "*" means absolute jmp */


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
