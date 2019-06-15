; Grub2で起動できたらなと作ったもの。(ELF式)
; Grub2が読み込んで処理してくれる

bits    32

global  boot_entry
extern  init_bios   ; at init_bios.asm
extern  init_efi64  ; at init_efi64.asm

; grub2のためのエントリー(multiboot2仕様)
; http://git.savannah.gnu.org/cgit/grub.git/tree/doc/multiboot2.h?h=multiboot2
MULTIBOOT_HEADER_MAGIC                    equ 0xe85250d6  ; 合言葉
MULTIBOOT_HEADER_ARCH                     equ 0           ; 4ならmips
MULTIBOOT_HEADER_LEN                      equ multiboot_end - multiboot_start
MULTIBOOT_HEADER_CHECKSUM                 equ -(MULTIBOOT_HEADER_MAGIC + MULTIBOOT_HEADER_ARCH + MULTIBOOT_HEADER_LEN)
MULTIBOOT_HEADER_FLAG                     equ 1           ; タグで使うフラグ(1はオプショナルを表す...?)
MULTIBOOT_HEADER_TAG_TYPE_FB              equ 5           ; フレームバッファ要求タグ
MULTIBOOT_HEADER_TAG_TYPE_EFI             equ 7           ; EFI サービスを終了させないようにする
MULTIBOOT_HEADER_TAG_ENTRY_ADDRESS_EFI64  equ 9           ; EFI64で最初に実行するアドレス
MULTIBOOT_HEADER_TAG_END                  equ 0           ; マルチブート用ヘッダー 定数定義終了(実体は下)
;========================================

; マルチブート用ヘッダー
section .multiboot_header  ; 特殊な扱いのセクションにする(配置固定 & 最適化無効)

jmp     boot_entry     ; 下を実行されたらまずいのでjmp

align   8

multiboot_start:
  dd      MULTIBOOT_HEADER_MAGIC
  dd      MULTIBOOT_HEADER_ARCH
  dd      MULTIBOOT_HEADER_LEN
  dd      MULTIBOOT_HEADER_CHECKSUM

; ここに追加のタグを書く
multiboot_tags_start:
  ;自力でフォント描写ができないため現在は無効
  ;dw      MULTIBOOT_HEADER_TAG_TYPE_FB
  ;dw      MULTIBOOT_HEADER_FLAG   ; flags
  ;dd      20                      ; size(このタグのサイズ)(multiboot_tag_framebuffer_end - multiboot_tag_framebuffer)
  ;dd      1024                    ; width(1行の文字数)
  ;dd      768                     ; height(行数)
  ;dd      32                      ; depth(色深度)
  align   8                       ; タグは8バイト間隔で並ぶ必要がある
  ;dw      MULTIBOOT_HEADER_TAG_TYPE_EFI
  ;dw      MULTIBOOT_HEADER_FLAG   ; flags
  ;dd      8                       ; size
  align   8
  dw      MULTIBOOT_HEADER_TAG_ENTRY_ADDRESS_EFI64
  dw      MULTIBOOT_HEADER_FLAG
  dd      12
  dd      init_efi64
  align   8                       ; タグは8バイト間隔で並ぶ必要がある
  dw      MULTIBOOT_HEADER_TAG_END
  dw      MULTIBOOT_HEADER_FLAG   ; flags
  dd      8                       ; size
multiboot_tags_end:
multiboot_end:
; マルチブート用ヘッダー実体記述終了
;========================================

section .text

boot_entry:
  jmp init_bios
