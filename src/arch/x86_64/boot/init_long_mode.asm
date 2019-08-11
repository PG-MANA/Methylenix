; 雑な初期化
; おそらく16bitリアルモードでINITを呼んでも行ける...はず(EFIからのブートは別)
; IDTの設定が終わるまでCLIしたままにする。そうでないと割り込みが入って死ぬ。
bits 32


; GLOBAL, EXTERN
global init_long_mode
extern init_x86_64                                  ; at init_x86_64.asm
extern main_code_segment_descriptor, tss_descriptor, gdtr0 ; at common.asm
extern tss_descriptor_adress, tss, pd, pdpt, pml4

section .text

init_long_mode:
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
  mov   word [tss_descriptor_adress + 2],  ax
  shr   eax,  16
  mov   byte [tss_descriptor_adress + 4],  al
  mov   byte [tss_descriptor_adress + 7],  ah

  push  0                   ; 64bit POPのための準備
  push  main_code_segment_descriptor
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
  jz    cpuid_not_supported ; cpuid非対応
  mov   eax,  0x80000000    ; cpuid拡張モードは有効か(使用可能なeaxの最大値が返る)
  cpuid
  cmp   eax,  0x80000001
  jb    only_x86
  mov   eax,  0x80000001
  cpuid
  test  edx,  1 << 29       ; Long Mode Enable Bitをテスト(64bitモードが有効かどうか)
  ;(AMD64 Architecture Programmer’s Manual, Volume 2: System Programming - 14.8 Long-Mode Initialization Example)
  jz    only_x86

  ; ページング設定
  ; 1GBページング対応かどうか
  test  edx,  1 << 26
  jz    init_normal_paging

init_4level_paging:
  xor   ecx,  ecx             ; カウンタ
pdpte_setup:
  ; https://software.intel.com/sites/default/files/managed/a4/60/325384-sdm-vol-3abcd.pdf (第四章 LEVEL4-PAGING)
  ; 1GB単位メモリページング
  ; PML4->PDPだけで完結

  mov   eax,  0x100000      ; ページ一つが管理する区域(1GB)
  mul   ecx                 ; eax = eax * ecx
  or    eax,  0b10000011    ; P(物理メモリ上) + r/w(read and write) + huge(PDPTでHugeを立てると1GB単位) 0b:二進数
  mov   [pdpt + ecx * 8], eax
  inc   ecx
  cmp   ecx,  4
  jne   pdpte_setup
  jmp   pml4_setup

init_normal_paging:
  xor   ecx,  ecx             ; カウンタ

.pde_setup:
  ; 2MB単位メモリページング
  ; PML4->PDP->PDで完結

  mov   eax,  0x200000      ; ページ一つが管理する区域(2MB)(掛け算で 0MB~ => 4MB~=>8MB~と増える)
  mul   ecx                 ; eax = eax * ecx
  or    eax,  0b10000011    ; P(物理メモリ上) + r/w(read and write) + huge(PDEでHugeを立てると２MB単位) 0b:二進数
  mov   [pd + ecx * 8], eax ; 64bitごとの配置
  inc   ecx                 ; ecx++
  cmp   ecx,  2048
  jne   .pde_setup          ; ecx != 512 * 4

  xor   ecx, ecx            ; カウンタ

.pdpte_setup_2mb:
  mov   eax,  4096
  mul   ecx
  add   eax,  pd            ; この３つで eax = 4096 * ecx + pdしてる
  or    eax,  0b11          ; P(物理メモリ上) + r/w(read and write)
  mov   [pdpt + ecx * 8], eax
  inc   ecx
  cmp   ecx,  4
  jne   .pdpte_setup_2mb

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
  or    eax,  1 << 8 | 1 << 11
  wrmsr                     ; LMEとNXEフラグを立てる
  mov   eax,  cr0
  or    eax,  1 << 31 | 1   ; PGフラグを立てる("|1"は既に32bitになってる場合は不要)
  mov   cr0,  eax           ; これらの初期化で4GBは仮想メモリアドレスと実メモリアドレスが一致しているはず。(ストレートマッピング)
  mov   ax,   tss_descriptor
  lgdt  [gdtr0]
  ltr   ax                  ; ホントは16bitから32bitになったときはすぐジャンプすべき
  jmp   main_code_segment_descriptor:init_x86_64

only_x86:                   ; 今の所下と同じ
cpuid_not_supported:
; とりあえずデバッグ
  mov   ecx,  error_str.end-error_str
  mov   edi,  0xb8000
  mov   esi,  error_str
  rep   movsb               ; 転送
  cli
  hlt
  jmp cpuid_not_supported


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
