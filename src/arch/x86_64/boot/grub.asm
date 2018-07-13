;GRUB2で起動できたらなと作ったもの。(ELF式)
;
bits    32

global  boot_entry
global  stack_top
extern  init;init.asm

;grub2のためのエントリー(multiboot2仕様)
; http://git.savannah.gnu.org/cgit/grub.git/tree/doc/multiboot2.h?h=multiboot2
multiboot_header_magic          equ 0xe85250d6 ; 合言葉
multiboot_header_arch           equ 0;4ならmips
multiboot_header_len            equ multiboot_end - multiboot_start;ヘッダの長さ
multiboot_header_checksum       equ 0x100000000-(multiboot_header_magic + multiboot_header_arch + multiboot_header_len)
multiboot_bootloader_magic      equ 0x36d76289


;grub2のタグ

multiboot_header_tag_end    equ 0;終了


;その他定数
stack_size   equ 256;初期のスタックサイズ(64bit化後変更になる)

;end
;========================================


;========================================
;マルチブート用ヘッダー
jmp     boot_entry;下を実行されたらまずいのでjmp
section .grub_header ;特殊な扱い
align   4;32bit
multiboot_start:
    dd  multiboot_header_magic
    dd  multiboot_header_arch
    dd  multiboot_header_len
    dd  multiboot_header_checksum

;ここにタグを書く
multiboot_tag_end:
    dw  multiboot_header_tag_end
    dw  0;flags
    dd  8;size
multiboot_end:
;マルチブート用ヘッダー記述終了
;========================================


;========================================
;textセクション
section .text
align   4
boot_entry:;起動処理のキモ
mov     esp,stack_top;スタック設定
;eflags初期化
push    0
popf
push    0;64bit popのための準備
push    ebx;アドレス保存

;eaxとebxはいじってはいけない
cmp     eax,multiboot_bootloader_magic
jne     bad_magic
jmp     init;初期化

bad_magic:
    mov ecx,error_str.end-error_str
    mov edi,0xb8000
    mov esi,error_str
    rep movsb;転送
    cli
    hlt
jmp bad_magic


section .data
align   4
error_str:
    ;dw 0x4f52と書くがエンディアンで分けるときは逆に書く
    db  'E',0x4f;4f:赤字に白文字
    db  'r',0x4f
    db  'r',0x4f
    db  'o',0x4f
    db  'r',0x4f
    db  ':',0x4f
    db  'M',0x4f
    db  'u',0x4f
    db  'l',0x4f
    db  't',0x4f
    db  'i',0x4f
    db  'b',0x4f
    db  'o',0x4f
    db  'o',0x4f
    db  't',0x4f
    db  ' ',0x4f
    db  'i',0x4f
    db  's',0x4f
    db  ' ',0x4f
    db  'n',0x4f
    db  'o',0x4f
    db  't',0x4f
    db  ' ',0x4f
    db  's',0x4f
    db  'u',0x4f
    db  'p',0x4f
    db  'p',0x4f
    db  'o',0x4f
    db  'r',0x4f
    db  't',0x4f
    db  'e',0x4f
    db  'd',0x4f
    db  '.',0x4f
.end:


;スタック
section .bss
align   4
stack_bottom:
resb    stack_size
stack_top:
