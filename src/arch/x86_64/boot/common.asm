; Stackなどの共通部分

; 定数
IO_MAP_SIZE equ 0xffff
STACK_SIZE  equ 256

global initial_stack, gdt_main_code, gdtr0
global tss_definition, tss_address_definition, tss


section .bss

align   4096

tss_stack:
  resb    4096

resb  STACK_SIZE
initial_stack:

section .data

align   8

gdt:
  .all:
    dq    0                       ; GDT云々するとき下位3にセグメント番号がかぶらないため、わざと0エントリを立てる。

gdt_main_code: equ $ - gdt
    ; すべてコードセグメント
    dq    (1 << 43) | (1 << 44) | (1 << 47) | (1 << 53)

tss_definition: equ $ - gdt
tss_address_definition:
    dw    tss.end - tss           ; Limit(Low)
    dw    0                       ; Base(Low)
    db    0                       ; Base(middle)
    db    10001001b               ; 64ビットTSS + DPL:0 + P:1
    db    0                       ; Limit(High)+Granularity
    db    0                       ; Base(Middle high)
    dd    0                       ; Base(High)
    dw    0                       ; 予約
    dw    0                       ; 予約

gdtr0:
  dw    $ - gdt - 1
  dq    gdt

align   4096

tss:
tss_address: equ $                   ; tss_addressをうまく使えないか
  dd    0
  dd    tss_stack
  dd    0
  times 22                dd    0
  dd    104 << 16

.end:
  times IO_MAP_SIZE / 8   db    0
