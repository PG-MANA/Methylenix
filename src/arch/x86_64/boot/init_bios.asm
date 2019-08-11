; BIOS場合の初期化コード
; EFI32かBIOSから起動したGrubで呼ばれる。

bits 32

global init_bios
extern init_long_mode ; at init_long_mode.asm
extern initial_stack  ; at common.asm

; 定数
MULTIBOOT_CHECK_MAGIC equ 0x36d76289  ; 正常に処理されたのであれば、EAXに代入されている値


section .text
align 4
init_bios:                 ; 起動処理のキモ
  mov   esp, initial_stack ; スタック設定

  ; eflags初期化
  push  0
  popfd
  push  0             ; 64bit popのための準備
  push  ebx           ; アドレス保存

  ; eaxとebxはいじってはいけない
  cmp   eax, MULTIBOOT_CHECK_MAGIC
  jne   bad_magic
  jmp   init_long_mode; 初期化( init_long_mode.asmへ )

bad_magic:
  mov   ecx, error_str.end-error_str
  mov   edi, 0xb8000
  mov   esi, error_str
  rep   movsb         ; 転送
  cli
  hlt
jmp bad_magic


section .data

align   4

error_str:
  ; dw 0x4f52と書くがエンディアンで分けるときは逆に書く
  db  'E', 0x4f;4f:赤字に白文字
  db  'r', 0x4f
  db  'r', 0x4f
  db  'o', 0x4f
  db  'r', 0x4f
  db  ':', 0x4f
  db  'M', 0x4f
  db  'u', 0x4f
  db  'l', 0x4f
  db  't', 0x4f
  db  'i', 0x4f
  db  'b', 0x4f
  db  'o', 0x4f
  db  'o', 0x4f
  db  't', 0x4f
  db  ' ', 0x4f
  db  'i', 0x4f
  db  's', 0x4f
  db  ' ', 0x4f
  db  'n', 0x4f
  db  'o', 0x4f
  db  't', 0x4f
  db  ' ', 0x4f
  db  's', 0x4f
  db  'u', 0x4f
  db  'p', 0x4f
  db  'p', 0x4f
  db  'o', 0x4f
  db  'r', 0x4f
  db  't', 0x4f
  db  'e', 0x4f
  db  'd', 0x4f
  db  '.', 0x4f
.end:
