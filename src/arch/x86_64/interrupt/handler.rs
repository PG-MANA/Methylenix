//!
//! # Interrupt Handler Maker
//!
//! This module is a macro to make interrupt handler.
//! interrupt handler must save/restore registers, but it is difficult with the Rust code.
//! This handler includes assembly code to do that.
//!
//! ## make_device_interrupt_handler($handler_name:ident, $handler_func:path)
//!
//! A macro rule to wrap normal handler with save/restore registers.
//!
//! This macro is used to device interruption.
//!
//!  * handler_name: wrapped function's name. it is used to register into InterruptManager.
//!  * handler_func: handler written in Rust,
//!                  the function made by this macro will call handler_func after save registers.
//!
//! ## make_error_interrupt_handler($handler_name: ident, $handler_func: path)
//!
//! A macro rule to wrap normal handler with save/restore registers.
//!
//! This macro is used to exception interruption. the error code will be passed to handler_func.
//!
//!  * handler_name: wrapped function's name. it is used to register into InterruptManager.
//!  * handler_func: handler written in Rust,
//!                  the function made by this macro will call handler_func after save registers.
//!                  the this function can take one argument: error code from CPU.
//!

#[macro_export]
macro_rules! make_device_interrupt_handler {
    ($handler_name:ident, $handler_func:path) => {
        #[naked]
        pub unsafe fn $handler_name() {
            llvm_asm!("
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
                mov     rbp, rsp":::: "intel","volatile");
            llvm_asm!(
                "call    $0"
                ::"X"($handler_func as unsafe fn() as *const unsafe fn())
                :: "intel","volatile");
            llvm_asm!("    
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
                iretq":::: "intel","volatile");
        }
    };
}

#[macro_export]
macro_rules! make_error_interrupt_handler {
    ($handler_name: ident, $handler_func: path) => {
        #[naked]
        pub unsafe fn $ handler_name() {
            llvm_asm!("
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
