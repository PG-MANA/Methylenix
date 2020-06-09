/*
 * common data for boot
 */

.equ IO_MAP_SIZE,           0xffff
.equ INITIAL_STACK_SIZE,    0x100
.equ TSS_STACK_SIZE,        0x100

.global initial_stack,INITIAL_STACK_SIZE, gdtr0
.global main_code_segment_descriptor, user_code_segment_descriptor, user_data_segment_descriptor
.global tss_descriptor,tss_descriptor_adress, tss
.global pd, pdpt, pml4

.section .bss

/* ページングディレクトリ(8byte * 512) * 4 */
.comm pd, 0x4000, 0x1000

/* ページディレクトリポインタテーブル(8byte * 512) */
.comm pdpt, 0x4000, 0x1000

/*  ページマップレベル4(8byte * 512) */
.comm pml4, 0x4000, 0x1000

/* 初期スタック (スタックを設定する時は+INITIAL_STACK_SIZEをする) */
.comm initial_stack, INITIAL_STACK_SIZE, 8

/* TSSスタック */
.comm tss_stack, TSS_STACK_SIZE, 8


.section .data

.align   16

gdt:
    /* GDT云々するとき下位3にセグメント番号がかぶらないため、わざと0エントリを立てる。 */
    .quad    0

.equ  main_code_segment_descriptor, . - gdt
    .quad    (1 << 41) | (1 << 43) | (1 << 44) | (1 << 47) | (1 << 53)

.equ user_code_segment_descriptor, . - gdt
    .quad    (1 << 41) | (1 << 43) | (1 << 44) | (3 << 45) | (1 << 47) | (1 << 53)

.equ user_data_segment_descriptor, . - gdt
    .quad    (1 << 41) | (1 << 44) | (3 << 45) | (1 << 47)| (1 << 53)

tss_descriptor_adress:
.equ  tss_descriptor, tss_descriptor_adress - gdt
    .word    (tss_end - tss) & 0xffff   /* Limit(Low) */
    .word    0                          /* Base(Low) */
    .byte    0                          /* Base(middle) */
    .byte    0b10001001                 /* 64ビットTSS + DPL:0 + P:1 */
    .byte    (tss_end - tss) & 0xff0000 /* Limit(High)+Granularity */
    .byte    0                          /* Base(Middle high) */
    .long    0                          /* Base(High) */
    .word    0                          /* 予約 */
    .word    0                          /* 予約 */

gdtr0:
  .word    . - gdt - 1                  /* リミット数(すぐ上がGDTなので計算で求めてる) */
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
