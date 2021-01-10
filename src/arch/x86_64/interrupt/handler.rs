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
//!                  handler_func should not expand inline of the wrapped function, you should use #[inline(never)]
//!
//! ## make_context_switch_interrupt_handler($handler_name: ident, $handler_func: path)
//!
//! A macro rule to wrap the device handler which may switch contexts with save/restore registers.
//!
//! This macro is used to device interruption.
//!
//!  * handler_name: wrapped function's name. it is used to register into InterruptManager.
//!  * handler_func: handler written in Rust,
//!                  the function made by this macro will call handler_func after save registers.
//!                  handler_func should not expand inline of the wrapped function, you should use #[inline(never)]
//!                  the function will be passed the address of ContextData.
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
//!                  handler_func should not expand inline of the wrapped function, you should use #[inline(never)]
//!

/// A macro rule to wrap normal handler with save/restore registers.
///
/// This macro is used to device interruption.
///
///  * handler_name: wrapped function's name. it is used to register into InterruptManager.
///  * handler_func: handler written in Rust,
///                  the function made by this macro will call handler_func after save registers.
///                  handler_func should not expand inline of the wrapped function, you should use #[inline(never)]
///
#[macro_export]
macro_rules! make_device_interrupt_handler {
    ($handler_name:ident, $handler_func:path) => {
        #[naked]
        pub unsafe extern fn $handler_name() {
            asm!(
                "
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
                sub     rsp, 512
                fxsave  [rsp]
                mov     rax, cs
                cmp     [rsp + 8 * 15 + 512 + 8], rax 
                je      1f
                swapgs
              1:
                mov     rbp, rsp
                call    {}
                mov     rsp, rbp
                mov     rax, cs
                cmp     [rsp + 8 * 15 + 512 + 8], rax 
                je      2f
                swapgs
              2:
                fxrstor [rsp]
                add     rsp, 512
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
                iretq", sym $handler_func, options(noreturn)
            );
        }
    };
}

/// A macro rule to wrap the device handler which may switch contexts with save/restore registers.
///
/// This macro is used to device interruption.
///
///  * handler_name: wrapped function's name. it is used to register into InterruptManager.
///  * handler_func: handler written in Rust,
///                  the function made by this macro will call handler_func after save registers.
///                  handler_func should not expand inline of the wrapped function, you should use #[inline(never)]
///                  the function will be passed the address of ContextData.
///
#[macro_export]
macro_rules! make_context_switch_interrupt_handler {
    ($handler_name:ident, $handler_func:path) => {
        #[naked]
        pub unsafe extern fn $handler_name() {
            asm!("
                sub     rsp, ({1} + 1) * 8 // +1 is for stack alignment
                mov     [rsp +  0 * 8] ,rax
                mov     [rsp +  1 * 8], rdx
                mov     [rsp +  2 * 8], rcx
                mov     [rsp +  3 * 8], rbx
                mov     [rsp +  4 * 8], rbp
                mov     [rsp +  5 * 8], rsi
                mov     [rsp +  6 * 8], rdi
                mov     [rsp +  7 * 8], r8
                mov     [rsp +  8 * 8], r9
                mov     [rsp +  9 * 8], r10
                mov     [rsp + 10 * 8], r11
                mov     [rsp + 11 * 8], r12
                mov     [rsp + 12 * 8], r13
                mov     [rsp + 13 * 8], r14
                mov     [rsp + 14 * 8], r15     
                xor     rax, rax
                mov     ax, ds
                mov     [rsp + 15 * 8], rax            
                mov     ax, fs
                mov     [rsp + 16 * 8], rax
                rdfsbase rax
                mov     [rsp + 17 * 8], rax
                mov     ax, gs
                mov     [rsp + 18 * 8], rax
                rdgsbase rax
                mov     [rsp + 19 * 8], rax
                mov     ax, es
                mov     [rsp + 20 * 8], rax
                mov     ax, ss
                mov     [rsp + 21 * 8], rax
                mov     rax, [rsp + (3 + ({1} + 1)) * 8]   // RSP
                mov     [rsp + 22 * 8], rax
                mov     rax, [rsp + (2 + ({1} + 1)) * 8]   // RFLAGS
                mov     [rsp + 23 * 8], rax
                mov     rax, [rsp + (1 + ({1} + 1)) * 8]   // CS
                mov     [rsp + 24 * 8], rax
                mov     rax, [rsp + (0 + ({1} + 1)) * 8]   // RIP
                mov     [rsp + 25 * 8], rax
                mov     rax, cr3
                mov     [rsp + 26 * 8], rax
                sub     rsp, 512
                fxsave  [rsp]
                mov     rax, cs
                cmp     [rsp + 512 +  ({1} + 1) * 8 + 8], rax
                je      1f
                swapgs
            1:
                mov     rbp, rsp
                mov     rdi, rsp
                call    {0}
                mov     rsp, rbp
                mov     rax, cs
                cmp     [rsp + 512 +  ({1} + 1) * 8 + 8], rax
                je      2f
                swapgs
            2:
                fxrstor [rsp]
                add     rsp, 512
                // Ignore CR3, RIP, CS, RFLAGS, RSP, DS, SS, GS, ES, FS
                mov     rax, [rsp +  0 * 8]
                mov     rdx, [rsp +  1 * 8]
                mov     rcx, [rsp +  2 * 8]
                mov     rbx, [rsp +  3 * 8]
                mov     rbp, [rsp +  4 * 8]
                mov     rsi, [rsp +  5 * 8]
                mov     rdi, [rsp +  6 * 8]
                mov     r8,  [rsp +  7 * 8]
                mov     r9,  [rsp +  8 * 8]
                mov     r10, [rsp +  9 * 8]
                mov     r11, [rsp + 10 * 8]
                mov     r12, [rsp + 11 * 8]
                mov     r13, [rsp + 12 * 8]
                mov     r14, [rsp + 13 * 8]
                mov     r15, [rsp + 14 * 8] 
                add     rsp, ({1} + 1) * 8
                iretq", sym $handler_func,
                const crate::arch::target_arch::context::context_data::ContextData::NUM_OF_REGISTERS,
                options(noreturn));
        }
    };
}

/// A macro rule to wrap normal handler with save/restore registers.
///
/// This macro is used to exception interruption. the error code will be passed to handler_func.
///
///  * handler_name: wrapped function's name. it is used to register into InterruptManager.
///  * handler_func: handler written in Rust,
///                  the function made by this macro will call handler_func after save registers.
///                  the this function can take one argument: error code from CPU.
///                  handler_func should not expand inline of the wrapped function, you should use #[inline(never)]
///
#[macro_export]
macro_rules! make_error_interrupt_handler {
    ($handler_name: ident, $handler_func: path) => {
        #[naked]
        pub unsafe extern fn $ handler_name() {
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
                mov     rax, cs
                cmp     [rsp + 8 * 15 + 512 + 8], rax 
                je      1f
                swapgs
              1:
                mov     rbp, rsp
                call    {}
                mov     rsp, rbp
                mov     rax, cs
                cmp     [rsp + 8 * 15 + 512 + 8], rax 
                je      2f
                swapgs
              2:
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
                iretq", sym $handler_func, options(noreturn));
        }
    };
}
