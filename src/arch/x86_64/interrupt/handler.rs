/*
 * Interrupt Handler Maker
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
                push    rdi
                push    rbp
                push    r8
                push    r9
                push    r10
                push    r11
                push    r12
                push    r13
                push    r14
                push    r15
                mov     rbp, rsp
                call    $0
                mov     rsp, rbp
                pop     r15
                pop     r14
                pop     r13
                pop     r12
                pop     r11
                pop     r10
                pop     r9
                pop     r8
                pop     rbp
                pop     rdi
                pop     rsi
                pop     rdx
                pop     rcx
                pop     rbx
                pop     rax
                iretq" ::"X"($handler_func as unsafe fn()):: "intel","volatile");
        }
    };
}

#[macro_export]
macro_rules! make_error_interrupt_hundler {
    ($handler_name: ident, $handler_func: path) => {
        #[naked]
        pub unsafe fn $ handler_name() {
            asm!("
                push    rdi
                mov     rdi, [rsp + 8]
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
                push    r15
                mov     rbp, rsp
                call    $0
                mov     rsp, rbp
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
                pop     rdi
                add     rsp, 8
                iretq"::"X"( $handler_func as  fn (usize))::"intel", "volatile");
        }
    };
}
