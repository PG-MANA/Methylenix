; Stackなどの共通部分

; 定数
IO_MAP_SIZE equ 0xffff
STACK_SIZE  equ 256

global initial_stack, gdtr0
global main_code_segment_descriptor, user_code_segment_descriptor, user_data_segment_descriptor
global tss_descriptor,tss_descriptor_adress, tss
global pd, pdpt, pml4

section .bss

align   4096

tss_stack:
  resb    4096

pd:
  ; ページングディレクトリ(8byte * 512) * 4
  resb    4096 * 4
pdpt:
  ; ページディレクトリポインタテーブル(8byte * 512)
  resb    4096
pml4:
  ; ページマップレベル4(8byte * 512)
  resb    4096

; 初期スタック
resb  STACK_SIZE
initial_stack:

section .data

align   8

gdt:
  dummy:
    dq    0                         ; GDT云々するとき下位3にセグメント番号がかぶらないため、わざと0エントリを立てる。

main_code_segment_descriptor: equ $ - gdt
    dq    (1 << 41) | (1 << 43) | (1 << 44) | (1 << 47) | (1 << 53)

user_code_segment_descriptor: equ $ - gdt
    dq    (1 << 41) | (1 << 43) | (1 << 44) | (3 << 45) | (1 << 47) | (1 << 53)

user_data_segment_descriptor: equ $ - gdt
    dq    (1 << 41) | (1 << 44) | (3 << 45) | (1 << 47)| (1 << 53)

tss_descriptor: equ $ - gdt
tss_descriptor_adress:
    dw    (tss.end - tss) & 0xffff  ; Limit(Low)
    dw    0                         ; Base(Low)
    db    0                         ; Base(middle)
    db    10001001b                 ; 64ビットTSS + DPL:0 + P:1
    db    (tss.end - tss) & 0xff0000; Limit(High)+Granularity
    db    0                         ; Base(Middle high)
    dd    0                         ; Base(High)
    dw    0                         ; 予約
    dw    0                         ; 予約

gdtr0:
  dw    $ - gdt - 1                 ; リミット数(すぐ上がGDTなので計算で求めてる)
  dq    gdt

align   4096

tss:
  dd    0
  dd    tss_stack
  dd    0
  times 22                dd    0
  dd    104 << 16
  times IO_MAP_SIZE / 8   db    0
.end:
