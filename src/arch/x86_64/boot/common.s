/*
 * Common data for boot
 */

.equ IO_MAP_SIZE,               0xffff
.equ OS_STACK_SIZE,             0x10000
.equ DIRECT_MAP_START_ADDRESS,  0xffffa00000000000
.equ KERNEL_MAP_START_ADDRESS,  0xffffff8000000000

.global os_stack, OS_STACK_SIZE, gdt, gdtr_64bit_0, gdtr_64bit_1, KERNEL_MAP_START_ADDRESS
.global main_code_segment_descriptor, user_code_segment_descriptor, user_data_segment_descriptor
.global tss_descriptor, tss_descriptor_address, tss
.global pd, pdpt, pml4

.section .data.32

.align  0x1000
/* PAGE DIRECTORY (8byte * 512) * 4 */
pd:
.skip   0x4000

/* PAGE DIRECTORY POINTER TABLE (8byte * 512[4 entries are used]) */
pdpt:
.skip   0x1000

/* PML4 (8byte * 512[1 entry is used]) */
pml4:
.skip   0x1000

.align   16
gdtr_64bit_0:
  .word    gdt_end - gdt - 1                  /* The byte size of descriptors */
  .quad    gdt - KERNEL_MAP_START_ADDRESS

.align   16
gdtr_64bit_1:
  .word    gdt_end - gdt - 1                  /* The byte size of descriptors */
  .quad    gdt

.section .data

.align   16
/* OS STACK */
os_stack:
.skip   OS_STACK_SIZE

.align   16
gdt:
    /* NULL DESCRIPTOR */
    .quad    0

.equ  main_code_segment_descriptor, . - gdt
    .quad    (1 << 41) | (1 << 43) | (1 << 44) | (1 << 47) | (1 << 53)

.equ main_data_segment_descriptor, . - gdt
    .quad    (1 << 41) | (1 << 44) | (1 << 47) | (1 << 53)

.equ user_data_segment_descriptor, . - gdt
    .quad    (1 << 41) | (1 << 44) | (3 << 45) | (1 << 47) | (1 << 53)

.equ user_code_segment_descriptor, . - gdt
    .quad    (1 << 41) | (1 << 43) | (1 << 44) | (3 << 45) | (1 << 47) | (1 << 53)

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
gdt_end:

.align   16

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
