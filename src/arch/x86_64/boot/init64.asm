 ;
;Copyright 2017 PG_MANA
;
;This software is Licensed under the Apache License Version 2.0
;See LICENSE.md
;
;雑な初期化(init.asmの続き)
;IDTの設定が終わるまでCLIしたままにする。そうでないと割り込みが入って死ぬ。
bits 64

section .text

global init64
extern boot_main

init64:
    ;セグメントレジスタ初期化、間違ってもCSはいじるな(FS,GSはマルチスレッドで使用する可能性がある...らしい)
    xor rax,rax
    mov es,ax
    mov ss,ax
    mov ds,ax
    mov fs,ax
    mov gs,ax
    pop rsi;RDI=>RSI=> RDX=>RCX=>R8=>...=>R9が引数リスト
    pop rdi;32bitでpushして64bitでpopはまずい気がする。
    mov rsp,stack_top;スタック設定
    jmp boot_main;//各アーキのbootに入ってる

;スタック
section .bss
align   4
stack_bottom:
resb    4096 * 10
stack_top:
