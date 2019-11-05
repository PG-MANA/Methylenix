; 雑な初期化(init.asmの続き)
; IDTの設定が終わるまでCLIしたままにする。そうでないと割り込みが入って死ぬ。
bits 64

; GLOBAL,EXTERN
global init_x86_64
extern boot_main
extern main_code_segment_descriptor, user_code_segment_descriptor, user_data_segment_descriptor

section .text

init_x86_64:
  ; セグメントレジスタ初期化、間違ってもCSはいじるな(FS,GSはマルチスレッドで使用する可能性がある...らしい)
  xor   rax,  rax
  mov   es,   ax
  mov   ss,   ax
  mov   ds,   ax
  mov   fs,   ax
  mov   gs,   ax
  pop   rdi             ; こうPOPすることでRDIにMultiBootInformationのアドレスが入る
  mov   rsi, main_code_segment_descriptor
  mov   rdx, user_code_segment_descriptor
  mov   rcx, user_data_segment_descriptor
  mov   rsp,  stack_top ; スタック設定
  jmp   boot_main       ; src/arch/x86_64/mod.rs


section .bss

align   4

stack_bottom:
  resb  4096 * 10
stack_top:
