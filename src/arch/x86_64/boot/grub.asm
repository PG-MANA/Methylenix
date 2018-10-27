;GRUB2で起動できたらなと作ったもの。(ELF式)
bits    32

global  boot_entry
global  stack_top
extern  init  ; init.asm

; grub2のためのエントリー(multiboot2仕様)
; http://git.savannah.gnu.org/cgit/grub.git/tree/doc/multiboot2.h?h=multiboot2
MULTIBOOT_HEADER_MAGIC          equ 0xe85250d6  ; 合言葉
MULTIBOOT_HEADER_ARCH           equ 0           ; 4ならmips
MULTIBOOT_HEADER_LEN            equ multiboot_end - multiboot_start
MULTIBOOT_HEADER_CHECKSUM       equ 0x100000000-(MULTIBOOT_HEADER_MAGIC + MULTIBOOT_HEADER_ARCH + MULTIBOOT_HEADER_LEN)
MULTIBOOT_CHECK_MAGIC           equ 0x36d76289  ; 正常に処理されたのであれば、EAXに代入されている値
MULTIBOOT_HEADER_TAG_END        equ 0           ; マルチブート用ヘッダー 定数定義終了(実体は下)

; その他定数
stack_size                      equ 256         ; 初期のスタックサイズ(64bit化後変更になる)
;========================================


;========================================
; マルチブート用ヘッダー
section .grub_header  ; 特殊な扱いのセクションにする(配置固定 & 最適化無効)

jmp     boot_entry    ; 下を実行されたらまずいのでjmp

align   4

multiboot_start:
  dd  MULTIBOOT_HEADER_MAGIC
  dd  MULTIBOOT_HEADER_ARCH
  dd  MULTIBOOT_HEADER_LEN
  dd  MULTIBOOT_HEADER_CHECKSUM

; ここに追加のタグを書く
multiboot_tag_end:
  dw  MULTIBOOT_HEADER_TAG_END
  dw  0 ; flags
  dd  8 ; size
multiboot_end:
; マルチブート用ヘッダー実体記述終了
;========================================


;========================================
section .text
align 4
boot_entry:           ; 起動処理のキモ
  mov   esp, stack_top ; スタック設定

  ; eflags初期化
  push  0
  popf
  push  0             ; 64bit popのための準備
  push  ebx           ; アドレス保存

  ; eaxとebxはいじってはいけない
  cmp   eax, MULTIBOOT_CHECK_MAGIC
  jne   bad_magic
  jmp   init          ; 初期化( init.asmへ )

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

; スタック
section .bss

align   4

stack_bottom:
  resb  stack_size
stack_top:
