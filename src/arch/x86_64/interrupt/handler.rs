/*
 * Copyright 2017 PG_MANA
 *
 * This software is Licensed under the Apache License Version 2.0
 * See LICENSE.md
 *
 * アセンブリによる割り込みハンドラ、手短に済ませるべき
 * できるだけNasmなどを使いたくない
 */

#[macro_export]
macro_rules! make_interrupt_hundler {
    ($handler_name:ident, $handler_func:path) => {
        #[naked]
        pub unsafe fn $handler_name() {
            asm!("
                push    fs
                push    gs
                push    rax
                push    rcx
                push    rdx
                push    rbx
                push    rbp
                push    rsi
                push    rdi" :::: "intel");
            $handler_func();
            asm!("
                pop     rdi
                pop     rsi
                pop     rbp
                pop     rbx
                pop     rdx
                pop     rcx
                pop     rax
                pop gs
                pop fs
                iretq" :::: "intel");
        }
    };
}
