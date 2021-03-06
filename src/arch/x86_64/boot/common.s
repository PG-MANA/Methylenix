/*
 * Common data for boot
 */

.equ IO_MAP_SIZE,           0xffff
.equ OS_STACK_SIZE,         0x10000

.global os_stack, OS_STACK_SIZE, gdt, gdtr0
.global main_code_segment_descriptor, user_code_segment_descriptor, user_data_segment_descriptor
.global tss_descriptor, tss_descriptor_address, tss
.global pd, pdpt, pml4


/* PAGE DIRECTORY (8byte * 512) * 4 */
.comm pd, 0x4000, 0x1000

/* PAGE DIRECTORY POINTER TABLE (8byte * 512[4 entries are used]) */
.comm pdpt, 0x1000, 0x1000

/* PML4 (8byte * 512[1 entry is used]) */
.comm pml4, 0x1000, 0x1000

/* OS STACK */
.comm os_stack, OS_STACK_SIZE, 0x10

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
    .word    (tss_end - tss) & 0xffff               /* Limit(Low) */
    .word    0                                      /* Base(Low) */
    .byte    0                                      /* Base(middle) */
    .byte    0b10001001                             /* 64bit TSS + DPL:0 + P:1 */
    .byte    ((tss_end - tss) & 0xff0000) >> 0x10   /* Limit(High)+Granularity */
    .byte    0                                      /* Base(Middle high) */
    .long    0                                      /* Base(High) */
    .word    0                                      /* Reserved */
    .word    0                                      /* Reserved */

gdtr0:
  .word    . - gdt - 1                  /* The byte size of descriptors */
  .quad    gdt

.align   4096

tss:
  .rept     25
    .long    0
  .endr
  .word     0
  .word     tss_io_map - tss
tss_io_map:
  .rept     IO_MAP_SIZE / 8
    .byte   0xff
  .endr
  .byte     0xff   /* See 19.5.2 "I/O Permission Bit Map" Intel SDM Vol.1 */
tss_end:
