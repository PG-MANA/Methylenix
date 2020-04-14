; EFI64で起動された場合の初期化コード
; 64bit対応確認を省略し64bitアドレスを扱う
; IDTの設定が終わるまでCLIしたままにする。そうでないと割り込みが入って死ぬ。

bits 64

; GLOBAL
global init_efi64
extern main_code_segment_descriptor, tss_descriptor, gdtr0 ; at common.asm
extern tss_descriptor_adress, tss, pd, pdpt, pml4, initial_stack
extern pd, pdpt, pml4
extern init_x86_64

; 定数
MULTIBOOT_CHECK_MAGIC equ 0x36d76289  ; 正常に処理されたのであれば、EAXに代入されている値

section .text

init_efi64:
  ; MultiBootInformationの仕様を読む限り、メモリ全てがストレートマッピングされてるらしい
  mov   rsp, initial_stack ; スタック設定
  ; eflags初期化
  push  0
  popf
  push  rbx
  cmp   eax, MULTIBOOT_CHECK_MAGIC
  jne   bad_magic
  ; TSSセグメント情報書き込み
  mov   eax,  tss
  mov   word [tss_descriptor_adress + 2],  ax
  shr   eax,  16
  mov   byte [tss_descriptor_adress + 4],  al
  mov   byte [tss_descriptor_adress + 7],  ah
  ; ページングを設定し直す
  mov   eax,  0x80000001
  cpuid                 ; CPUID拡張モードの確認は省略
  test  edx,  1 << 26   ; 4 Level Pagingの確認
  jz    init_normal_paging
  jmp   init_4level_paging

bad_magic:
  hlt
  jmp bad_magic

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

  mov   ecx,  0             ; カウンタ

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

;reset:
  push  0                   ; mainがreturnしたときの戻り先(returnすることないので0にする)
  push main_code_segment_descriptor
  push init_x86_64
  mov   ecx,  0xc0000080    ; rdmsrのための準備(レジスタ指定)
  rdmsr                     ; モデル固有レジスタに記載(intelの場合pentium以降に搭載、cpuidで検査済)
  or    eax,  1 << 11
  wrmsr                     ; NXEフラグを立てる
  mov   rax,  pml4
  mov   cr3,  rax           ; PML4Eをcr3に設定する
  mov   ax,   tss_descriptor
  ltr   ax
  lgdt  [gdtr0]
  retfq
