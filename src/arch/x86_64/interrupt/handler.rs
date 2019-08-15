/*
アセンブリによる割り込みハンドラ、手短に済ませるべき
できるだけNasmなどを使いたくない
*/

#[macro_export]
macro_rules! make_interrupt_hundler {
    ($handler_name:ident, $handler_func:path) => {
        #[naked]
        pub unsafe fn $handler_name() {
            asm!("
                push    rax
                push    rbx
                push    rcx
                push    rdx
                push    rsi
                push    rbp
                push    r8
                push    r9
                push    r10
                push    r11
                push    r12
                push    r13
                push    r14
                push    r15" :::: "intel");
            $handler_func();
            asm!("
                pop     r15
                pop     r14
                pop     r13
                pop     r12
                pop     r11
                pop     r10
                pop     r9
                pop     r8
                pop     rbp
                pop     rsi
                pop     rdx
                pop     rcx
                pop     rbx
                pop     rax
                iretq" :::: "intel");
        }
    };
}

#[macro_export]
macro_rules! make_error_interrupt_hundler {
    ($handler_name:ident, $handler_func:path) => {
        #[naked]
        pub unsafe fn $handler_name() {
            let error_code: usize;
            asm!("
                pop     rdi
                push    rax
                push    rbx
                push    rcx
                push    rdx
                push    rsi
                push    rbp
                push    r8
                push    r9
                push    r10
                push    r11
                push    r12
                push    r13
                push    r14
                push    r15" :"={rdi}"(error_code)::: "intel");
            $handler_func(error_code);
            asm!("
                pop     r15
                pop     r14
                pop     r13
                pop     r12
                pop     r11
                pop     r10
                pop     r9
                pop     r8
                pop     rbp
                pop     rsi
                pop     rdx
                pop     rcx
                pop     rbx
                pop     rax
                push    rax
                iretq" :::: "intel");
        }
    };
}
