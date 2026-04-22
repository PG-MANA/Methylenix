/*
 * Boot entry for application processors
 */

.global ap_entry, ap_entry_end, ap_information_address

.extern main_code_segment_descriptor, gdtr_64bit_main   /* at common.asm */
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

    /* Check if the address was adjusted */
    mov    (ap_address_already_adjusted - ap_entry), %cl
    cmp     $0, %cl
    jne     1f

    /* Set jump address into ljmpl */
    add     %ebx, ljmpl_32_address - ap_entry   /* Add base address */

    /* Set GDT address into gdtr */
    add     %ebx, gdtr_32bit - ap_entry + 2

1:
    lgdt    (gdtr_32bit - ap_entry)

    mov     %cr0, %eax
    and     $0x7fffffff, %eax                   /* Disable paging */
    or      $0x00000001, %eax                   /* Enable 32bit protect mode */
    mov     %eax, %cr0

    /* Long JMP */
    .byte 0x66, 0xea                            /* opcode and 32bit address prefix */
ljmpl_32_address:
    .long (ap_setup_long_mode - ap_entry)       /* offset (base will be added before) */
    .word gdt_32bit_code_segment_descriptor     /* code segment */

.code32
ap_setup_long_mode:
    mov     $gdt_32bit_data_segment_descriptor, %ax
    mov     %ax, %ds

    /* Check if the address was adjusted */
    cmp     $0, %cl
    jne     2f

    /* Set jump address into ljmpl */
    mov     $(ljmpl_64_address - ap_entry), %eax
    add     %ebx, (%ebx, %eax)

    /* Set GDT address into gdtr */
    add     %ebx, (gdtr_64bit - ap_entry + 2)(%ebx)

2:
    /* Get the page table passed from BSP */
    mov     (ap_initial_page_table - ap_entry)(%ebx), %eax
    mov     %eax, %cr3

    /* Enable compatible mode */
    mov     %cr4, %eax
    or      $(1 << 5), %eax
    mov     %eax, %cr4                          /* Set PAE flag */
    mov     $0xc0000080, %ecx
    rdmsr
    or      $(1 << 8 | 1 << 11), %eax
    wrmsr                                       /* Set LME and NXE flags */
    mov     %cr0, %eax
    or      $(1 << 31 | 1), %eax                /* Set PG flag */
    lgdt    (gdtr_64bit - ap_entry)(%ebx)
    mov     %eax, %cr0

    /* Long JMP to long mode */
    .byte   0xea                                /* opcode */
ljmpl_64_address:
    .long   (ap_init_long_mode - ap_entry)      /* offset (base will be added before) */
    .word   gdt_64bit_code_segment_descriptor   /* code segment */

.code64
ap_init_long_mode:
    /* Set stack */
    mov     (ap_initial_stack - ap_entry)(%ebx), %rsp

    /* Set the mark that address already adjusted */
    movb    $1, (ap_address_already_adjusted - ap_entry)(%ebx)

    /* Clear segment registers */
    xor     %ax, %ax
    mov     %ax, %es
    mov     %ax, %ss
    mov     %ax, %ds
    mov     %ax, %fs
    mov     %ax, %gs

    /* Set main GDT and jump to entry point */
    pushq   $main_code_segment_descriptor
    movabs  $ap_boot_main, %rax
    push    %rax
    movabs  $gdtr_64bit_main, %rax
    lgdt    (%rax)
    lretq

.align      16
gdt_32bit:
    /* NULL DESCRIPTOR */
    .quad   0

.equ        gdt_32bit_code_segment_descriptor, . - gdt_32bit
    .word   0xffff, 0x0000, 0x9b00, 0x00cf

.equ        gdt_32bit_data_segment_descriptor, . - gdt_32bit
    .word   0xffff, 0x0000, 0x9200, 0x00cf

gdtr_32bit:
    .word   . - gdt_32bit - 1
    .long   gdt_32bit - ap_entry

.align      16
gdt_64bit:
    /* NULL DESCRIPTOR */
    .quad   0

.equ        gdt_64bit_code_segment_descriptor, . - gdt_64bit
    .quad   (1 << 41) | (1 << 43) | (1 << 44) | (1 << 47) | (1 << 53)

gdtr_64bit:
  .word     . - gdt_64bit - 1
  .quad     gdt_64bit - ap_entry

.align      8
ap_information_address:
ap_initial_stack:
    .quad   0
ap_initial_page_table:
    .quad   0
ap_address_already_adjusted:
    .byte   0
ap_entry_end:
.size       ap_entry, ap_entry_end - ap_entry
