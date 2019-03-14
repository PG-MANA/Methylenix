; EFI64で起動された場合の初期化コード
; 64bit対応確認を省略し64bitアドレスを扱う
; IDTの設定が終わるまでCLIしたままにする。そうでないと割り込みが入って死ぬ。

bits 64

; GLOBAL
global init_efi64
extern init_x86_64                          ; at init_x86_64.asm
extern initial_stack, gdt_main_code, gdtr0  ; at common.asm
extern tss_definition, tss_address_definition, tss

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
  mov   word [tss_address_definition + 2],  ax
  shr   eax,  16
  mov   byte [tss_address_definition + 4],  al
  mov   byte [tss_address_definition + 7],  ah

  push  gdt_main_code                           ; あとで使う
  ; GDTとTSSをセットし直す
  lgdt  [gdtr0]
  mov   ax,   tss_definition
  ltr   ax
  jmp   init_x86_64


bad_magic:
  hlt
  jmp bad_magic
