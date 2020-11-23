/*
 * Common data for boot
 */

.equ IO_MAP_SIZE,           0xffff
.equ INITIAL_STACK_SIZE,    0x100
.equ TSS_STACK_SIZE,        0x100
.equ OS_STACK_SIZE,         0x8000

.global initial_stack, INITIAL_STACK_SIZE, os_stack, OS_STACK_SIZE, gdt, gdtr0
.global main_code_segment_descriptor, user_code_segment_descriptor, user_data_segment_descriptor
.global tss_descriptor, tss_descriptor_address, tss
.global pd, pdpt, pml4

.section .bss

/* PAGE DIRECTPRY (8byte * 512) * 4 */
.comm pd, 0x4000, 0x1000

/* PAGE DIRECTPRY POINTER TABLE (8byte * 512[4 entries are used]) */
.comm pdpt, 0x1000, 0x1000

/* PML4 (8byte * 512[1 entry is used]) */
.comm pml4, 0x1000, 0x1000

/* OS STACK */
.comm os_stack, OS_STACK_SIZE, 0x1000

/* INITAL STACK (This stack is used until jump to the rust code.) */
.comm initial_stack, INITIAL_STACK_SIZE, 0x1000

/* TSS STACK */
.comm tss_stack, TSS_STACK_SIZE, 8


.section .data

.align   16

gdt:
    /* NULL DESCRIPTOR */
    .quad    0

.equ  main_code_segment_descriptor, . - gdt
    .quad    (1 << 41) | (1 << 43) | (1 << 44) | (1 << 47) | (1 << 53)

.equ user_code_segment_descriptor, . - gdt
    .quad    (1 << 41) | (1 << 43) | (1 << 44) | (3 << 45) | (1 << 47) | (1 << 53)

.equ user_data_segment_descriptor, . - gdt
    .quad    (1 << 41) | (1 << 44) | (3 << 45) | (1 << 47)| (1 << 53)

tss_descriptor_address:
.equ  tss_descriptor, tss_descriptor_address - gdt
    .word    (tss_end - tss) & 0xffff   /* Limit(Low) */
    .word    0                          /* Base(Low) */
    .byte    0                          /* Base(middle) */
    .byte    0b10001001                 /* 64bit TSS + DPL:0 + P:1 */
    .byte    (tss_end - tss) & 0xff0000 /* Limit(High)+Granularity */
    .byte    0                          /* Base(Middle high) */
    .long    0                          /* Base(High) */
    .word    0                          /* Reserved */
    .word    0                          /* Reserved */

gdtr0:
  .word    . - gdt - 1                  /* The byte size of descriptors */
  .quad    gdt

.align   4096

tss:
  .long    0
  .long    tss_stack + TSS_STACK_SIZE
  .long    0
  .rept    22
    .long    0
  .endr
  .long    104 << 16
  .rept    IO_MAP_SIZE / 8
    .byte    0
  .endr
tss_end:
