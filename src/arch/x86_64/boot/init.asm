; 雑な初期化
; おそらく16bitリアルモードでINITを呼んでも行ける...はず
; IDTの設定が終わるまでCLIしたままにする。そうでないと割り込みが入って死ぬ。
bits 32

; 定数定義
IO_MAP_SIZE equ 0xffff

; GLOBAL, EXTERN
global init
extern init64 ; at init64.asm


section .text

init:
  ; 条件を満たしたcpuか確認(64it化できるか)
  ; http://wiki.osdev.org/setting_up_long_mode#detection_of_cpuid
  ; http://softwaretechnique.jp/os_development/tips/ia32_instructions/cpuid.html
  mov   al, 0xff
  out   0x21, al
  nop                       ; OUT命令を連続させるとうまくいかない機種があるらしいので
  out   0xa1, al
  cli

  ; TSSセグメント情報書き込み
  mov   eax,  tss
  mov   word [gdt.tss_address_definition + 2],  ax
  shr   eax,  16
  mov   byte [gdt.tss_address_definition + 4],  al
  mov   byte [gdt.tss_address_definition + 7],  ah

  push  0                   ; 64bit POPのための準備
  push  gdt.main_code       ; あとで使う
  pushfd
  pop   eax
  mov   ecx,  eax           ; 比較用にとっておく
  xor   eax,  1 << 21       ; IDフラグを反転
  push  eax
  popfd
  pushfd
  pop   eax
  push  ecx
  popfd                     ; 元に戻す
  xor   eax,  ecx           ; 比較
  jz    nocpuid             ; cpuid非対応
  mov   eax,  0x80000000    ; cpuid拡張モードは有効か(使用可能なeaxの最大値が返る)
  cpuid
  cmp   eax,  0x80000001
  jb    x86
  mov   eax,  0x80000001
  cpuid
  test  edx,  1 << 29       ; LMビットをテスト(Intel64が有効かどうか) TODO: AMD64対応
  jz    x86

  ; ページング設定
  ; 1GBページング対応かどうか
  test  edx,  1 << 26
  jz    no1gb

pdpte_setup:
  ; https://software.intel.com/sites/default/files/managed/a4/60/325384-sdm-vol-3abcd.pdf (第四章 LEVEL4-PAGING)
  ; 1GB単位メモリページング
  ; PML4->PDPだけで完結

  mov   eax,  0             ; 先頭アドレスが管理する区域(1GB)(予約域は0で埋めている。)
  or    eax,  0b10000011    ; P(物理メモリ上) + r/w(read and write) + huge(PDPTでHugeを立てると1GB単位) 0b:二進数
  mov   [pdpt], eax
  jmp   pml4_setup

no1gb:
  mov   ecx,  0             ; カウンタ

.pde_setup:
  ; 2MB単位メモリページング
  ; PML4->PDP->PDで完結

  mov   eax,  0x200000      ; ページ一つが管理する区域(2MB)(掛け算で 0MB~ => 2MB~=>4MB~と増える)
  mul   ecx                 ; eax = eax * ecx
  or    eax,  0b10000011    ; P(物理メモリ上) + r/w(read and write) + huge(PDEでHugeを立てると２MB単位) 0b:二進数
  mov   [pd + ecx * 8], eax ; 64bitごとの配置
  inc   ecx                 ; ecx++
  cmp   ecx,  512
  jne   .pde_setup          ; ecx != 512

;.pdpte_setup:
  mov   eax,  pd
  or    eax,  0b11          ; P(物理メモリ上) + r/w(read and write)
  mov   [pdpt], eax         ; ページマップレベル4の最初に追加

pml4_setup:
  mov   eax,  pdpt
  or    eax,  0b11          ; P(物理メモリ上) + r/w(read and write)
  mov   [pml4], eax         ; ページマップレベル4の最初に追加

;setup_64:
  mov   eax,  pml4
  mov   cr3,  eax           ; PML4Eをcr3に設定する
  mov   eax,  cr4
  or    eax,  1 << 5
  mov   cr4,  eax           ; PAEフラグを立てる
  mov   ecx,  0xc0000080    ; rdmsrのための準備(レジスタ指定)
  rdmsr                     ; モデル固有レジスタに記載(intelの場合pentium以降に搭載、cpuidで検査済)
  or    eax,  1 << 8        ; lmフラグを立てる
  wrmsr
  mov   eax,  cr0
  or    eax,  1 << 31 | 1   ; PGフラグを立てる("|1"は既に32bitになってる場合は不要)
  mov   cr0,  eax           ; これらの初期化で1GBは仮想メモリアドレスと実メモリアドレスが一致しているはず。(ストレートマッピング)
  lgdt  [gdtr0]
  mov   ax,   gdt.tss_definition
  ltr   ax                  ; ホントは16bitから32bitになったときはすぐジャンプすべき
  jmp   gdt.main_code:init64

x86:                        ; 今の所下と同じ
nocpuid:
; とりあえずデバッグ
  mov   ecx,  error_str.end-error_str
  mov   edi,  0xb8000
  mov   esi,  error_str
  rep   movsb               ; 転送
  cli
  hlt
  jmp nocpuid


section .data

align   4

error_str:
  ; dw 0x4f52と書くがエンディアンで分けるときは逆に書く
  db   'E',   0x4f            ; 4f:赤字に白文字
  db   'r',   0x4f
  db   'r',   0x4f
  db   'o',   0x4f
  db   'r',   0x4f
  db   ':',   0x4f
  db   '6',   0x4f
  db   '4',   0x4f
  db   'b',   0x4f
  db   'i',   0x4f
  db   't',   0x4f
  db   ' ',   0x4f
  db   'l',   0x4f
  db   'o',   0x4f
  db   'n',   0x4f
  db   'g',   0x4f
  db   ' ',   0x4f
  db   'm',   0x4f
  db   'o',   0x4f
  db   'd',   0x4f
  db   'e',   0x4f
  db   ' ',   0x4f
  db   'i',   0x4f
  db   's',   0x4f
  db   ' ',   0x4f
  db   'n',   0x4f
  db   'o',   0x4f
  db   't',   0x4f
  db   ' ',   0x4f
  db   's',   0x4f
  db   'u',   0x4f
  db   'p',   0x4f
  db   'p',   0x4f
  db   'o',   0x4f
  db   'r',   0x4f
  db   't',   0x4f
  db   'e',   0x4f
  db   'd',   0x4f
  db   '.',   0x4f
.end:

align   8

gdt:
  .all:
    dq    0                       ;GDT云々するとき下位3にセグメント番号がかぶらないため、わざと0エントリを立てる。

  .main_code: equ $ - gdt
    ;すべてコードセグメント
    dq    (1 << 43) | (1 << 44) | (1 << 47) | (1 << 53)

  .tss_definition: equ $ - gdt
  .tss_address_definition:
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
  dd    stack
  dd    0
  times 22                dd    0
  dd    104 << 16

.end:
  times IO_MAP_SIZE / 8   db    0

section .bss

align   4096

stack:
  resb    4096

pd:
; ページングディレクトリ(8byte * 512)
  resb    4096

pdpt:
; ページディレクトリポインタテーブル(8byte * 512)
  resb    4096
pml4:
; ページマップレベル4(8byte * 512)
  resb    4096
