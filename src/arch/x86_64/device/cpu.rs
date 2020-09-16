//!
//! x86_64 Specific Instruction
//!
//! This module is the collection of inline assembly functions.
//! All functions are unsafe, please be careful.  
//!

use crate::arch::target_arch::context::context_data::ContextData;

#[inline(always)]
pub unsafe fn sti() {
    asm!("sti");
}

#[inline(always)]
pub unsafe fn cli() {
    asm!("cli");
}

#[inline(always)]
pub unsafe fn enable_interrupt() {
    sti();
}

#[inline(always)]
pub unsafe fn disable_interrupt() {
    cli();
}

#[inline(always)]
pub unsafe fn halt() {
    hlt();
}

#[inline(always)]
pub unsafe fn hlt() {
    asm!("hlt");
}

#[inline(always)]
pub unsafe fn out_byte(port: u16, data: u8) {
    asm!("out dx, al",in("dx") port, in("al") data);
}

#[inline(always)]
pub unsafe fn in_byte(port: u16) -> u8 {
    let mut result: u8;
    asm!("in al, dx",in("dx") port,out("al") result);
    result
}

/// Operate "in" twice.
///
/// This function is useful when you treat device returning 16bit data with 8bit register.
#[inline(always)]
pub unsafe fn in_byte_twice(port: u16) -> (u8 /*first*/, u8 /*second*/) {
    let mut r1: u8;
    let mut r2: u8;
    asm!("  in  al, dx
            mov bl, al
            in  al, dx    
    ", in("dx") port,out("bl") r1,out("al") r2);
    (r1, r2)
}

#[inline(always)]
pub unsafe fn in_dword(port: u16) -> u32 {
    let mut result: u32;
    asm!("in eax, dx", in("dx") port, out("eax") result);
    result
}

#[inline(always)]
pub unsafe fn sgdt(gdtr: &mut u128) {
    asm!("sgdt [{}]", in(reg) (gdtr as *const _ as usize));
}

#[inline(always)]
pub unsafe fn store_tr() -> u16 {
    let result: u16;
    asm!("str ax", out("ax") result);
    result
}

#[inline(always)]
pub unsafe fn lidt(idtr: usize) {
    asm!("lidt [{}]", in(reg) idtr);
}

#[inline(always)]
pub unsafe fn rdmsr(ecx: u32) -> u64 {
    let mut edx: u32;
    let mut eax: u32;
    asm!("rdmsr", in("ecx") ecx, out("edx") edx, out("eax") eax);
    (edx as u64) << 32 | eax as u64
}

#[inline(always)]
pub unsafe fn wrmsr(ecx: u32, data: u64) {
    let edx: u32 = (data >> 32) as u32;
    let eax: u32 = data as u32;
    asm!("wrmsr", in("eax") eax, in("edx") edx, in("ecx") ecx);
}

/// Operate "cpuid".
///
/// eax and ecx are used as selector, so you must set before calling this function.
/// The result will set into each argument.
#[inline(always)]
pub unsafe fn cpuid(eax: &mut u32, ebx: &mut u32, ecx: &mut u32, edx: &mut u32) {
    asm!(
        "cpuid",
        inout("eax") * eax,
        inout("ecx") * ecx,
        out("ebx") * ebx,
        out("edx") * edx
    );
}

#[inline(always)]
pub unsafe fn get_cr0() -> u64 {
    let mut result: u64;
    asm!("mov {}, cr0", out(reg) result);
    result
}

#[inline(always)]
pub unsafe fn set_cr0(cr0: u64) {
    asm!("mov cr0, {}",in(reg) cr0);
}

#[inline(always)]
pub unsafe fn set_cr3(address: usize) {
    asm!("mov cr3, {}", in(reg) address);
}

#[inline(always)]
pub unsafe fn get_cr3() -> usize {
    let mut result: u64;
    asm!("mov {}, cr3", out(reg) result);
    result as usize
}

#[inline(always)]
pub unsafe fn get_cr4() -> u64 {
    let mut result: u64;
    asm!("mov {}, cr4", out(reg) result);
    result
}

#[inline(always)]
pub fn is_interruption_enabled() -> bool {
    let r_flags: u64;
    unsafe {
        asm!("  pushfq
                pop {}", out(reg) r_flags)
    };
    (r_flags & (1 << 9)) != 0
}

#[inline(always)]
pub unsafe fn set_cr4(cr4: u64) {
    asm!("mov cr4, {}", in(reg) cr4);
}

#[inline(always)]
pub unsafe fn invlpg(address: usize) {
    asm!("invlpg [{}]", in(reg) address);
}

pub unsafe fn enable_sse() {
    let mut cr0 = get_cr0();
    cr0 &= !(1 << 2); /* clear EM */
    cr0 |= 1 << 1; /* set MP */
    set_cr0(cr0);
    let mut cr4 = get_cr4();
    cr4 |= (1 << 10) | (1 << 9); /*set OSFXSR and OSXMMEXCPT*/
    set_cr4(cr4);
}

/// Run ContextData.
///
/// This function is called from ContextManager.
/// Set all registers from context_data and jump context_data.rip.
/// This function assume 1st argument is passed by "rdi" and 2nd is passed by "rsi"
#[naked]
#[inline(never)]
#[allow(unused_variables)]
pub unsafe extern "C" fn run_task(context_data_address: *const ContextData) {
    asm!(
        "
                fxrstor [rdi]
                mov     rdx, [rdi + 512 + 8 *  1]
                mov     rcx, [rdi + 512 + 8 *  2]
                mov     rbx, [rdi + 512 + 8 *  3]
                mov     rbp, [rdi + 512 + 8 *  4]
                mov     rsi, [rdi + 512 + 8 *  5]
                mov     r8,  [rdi + 512 + 8 *  7]
                mov     r9,  [rdi + 512 + 8 *  8]
                mov     r10, [rdi + 512 + 8 *  9]
                mov     r11, [rdi + 512 + 8 * 10]
                mov     r12, [rdi + 512 + 8 * 11]
                mov     r13, [rdi + 512 + 8 * 12]
                mov     r14, [rdi + 512 + 8 * 13]
                mov     r15, [rdi + 512 + 8 * 14]
                mov     rax, [rdi + 512 + 8 * 15]
                mov     ds, ax
                mov     rax, [rdi + 512 + 8 * 16]
                mov     fs, ax
                mov     rax, [rdi + 512 + 8 * 17]
                mov     gs, ax
                mov     rax, [rdi + 512 + 8 * 18]
                mov     es, ax
                
                push    [rdi + 512 + 8 * 19] // SS
                push    [rdi + 512 + 8 * 20] // RSP
                push    [rdi + 512 + 8 * 21] // RFLAGS
                push    [rdi + 512 + 8 * 22] // CS
                push    [rdi + 512 + 8 * 23] // RIP

                mov     rax, [rdi + 512 + 8 * 24]
                mov     cr3, rax
                mov     rax, [rdi + 512]
                mov     rdi, [rdi + 512 + 8 *  6]
                iretq
                "
    );
}

/// Save current process into now_context_data and run next_context_data.
///
/// This function is called by ContextManager.
/// This function does not return until another process switches to now_context_data.
/// This function assume 1st argument is passed by "rdi" and 2nd is passed by "rsi".
#[naked]
#[inline(never)]
#[allow(unused_variables)]
pub unsafe extern "C" fn task_switch(
    next_context_data_address: *const ContextData,
    now_context_data_address: *mut ContextData,
) {
    asm!(
        "
                fxsave  [rsi]
                mov     [rsi + 512],          rax
                mov     [rsi + 512 + 8 *  1], rdx
                mov     [rsi + 512 + 8 *  2], rcx
                mov     [rsi + 512 + 8 *  3], rbx
                mov     [rsi + 512 + 8 *  4], rbp
                mov     [rsi + 512 + 8 *  5], rsi
                mov     [rsi + 512 + 8 *  6], rdi
                mov     [rsi + 512 + 8 *  7], r8
                mov     [rsi + 512 + 8 *  8], r9
                mov     [rsi + 512 + 8 *  9], r10
                mov     [rsi + 512 + 8 * 10], r11
                mov     [rsi + 512 + 8 * 11], r12
                mov     [rsi + 512 + 8 * 12], r13
                mov     [rsi + 512 + 8 * 13], r14
                mov     [rsi + 512 + 8 * 14], r15
                mov     rax, ds
                mov     [rsi + 512 + 8 * 15], rax
                mov     rax, fs
                mov     [rsi + 512 + 8 * 16], rax
                mov     rax, gs
                mov     [rsi + 512 + 8 * 17], rax
                mov     rax, es
                mov     [rsi + 512 + 8 * 18], rax
                mov     rax, ss
                mov     [rsi + 512 + 8 * 19], rax
                mov     rax, rsp
                mov     [rsi + 512 + 8 * 20], rax
                pushfq
                pop     rax
                mov     [rsi + 512 + 8 * 21], rax   // RFLAGS
                mov     rax, cs
                mov     [rsi + 512 + 8 * 22], rax
                lea     rax, 1f
                mov     [rsi + 512 + 8 * 23], rax   // RIP
                mov     rax, cr3
                mov     [rsi + 512 + 8 * 24], rax

                fxrstor [rdi]
                mov     rdx, [rdi + 512 + 8 *  1]
                mov     rcx, [rdi + 512 + 8 *  2]
                mov     rbx, [rdi + 512 + 8 *  3]
                mov     rbp, [rdi + 512 + 8 *  4]
                mov     rsi, [rdi + 512 + 8 *  5]
                mov     r8,  [rdi + 512 + 8 *  7]
                mov     r9,  [rdi + 512 + 8 *  8]
                mov     r10, [rdi + 512 + 8 *  9]
                mov     r11, [rdi + 512 + 8 * 10]
                mov     r12, [rdi + 512 + 8 * 11]
                mov     r13, [rdi + 512 + 8 * 12]
                mov     r14, [rdi + 512 + 8 * 13]
                mov     r15, [rdi + 512 + 8 * 14]
                mov     rax, [rdi + 512 + 8 * 15]
                mov     ds, ax
                mov     rax, [rdi + 512 + 8 * 16]
                mov     fs, ax
                mov     rax, [rdi + 512 + 8 * 17]
                mov     gs, ax
                mov     rax, [rdi + 512 + 8 * 18]
                mov     es, ax
                
                push    [rdi + 512 + 8 * 19] // SS
                push    [rdi + 512 + 8 * 20] // RSP
                push    [rdi + 512 + 8 * 21] // RFLAGS
                push    [rdi + 512 + 8 * 22] // CS
                push    [rdi + 512 + 8 * 23] // RIP

                mov     rax, [rdi + 512 + 8 * 24]
                mov     cr3, rax
                mov     rax, [rdi + 512]
                mov     rdi, [rdi + 512 + 8 *  6]
                iretq
                1:
                "
    );
}
