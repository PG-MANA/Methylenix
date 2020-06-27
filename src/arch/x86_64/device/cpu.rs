/*
 * x86_64 Specific Instruction
 */

use arch::target_arch::context::context_data::ContextData;

#[inline(always)]
pub unsafe fn sti() {
    llvm_asm!("sti");
}

/*
#[inline(always)]
pub unsafe fn cli() {
    llvm_asm!("cli");
}
*/

#[inline(always)]
pub unsafe fn halt() {
    hlt();
}

#[inline(always)]
pub unsafe fn hlt() {
    llvm_asm!("hlt");
}

#[inline(always)]
pub unsafe fn out_byte(addr: u16, data: u8) {
    llvm_asm!("outb %al, %dx"::"{dx}"(addr), "{al}"(data));
}

#[inline(always)]
pub unsafe fn in_byte(data: u16) -> u8 {
    let result: u8;
    llvm_asm!("in %dx, %al":"={al}"(result):"{dx}"(data)::"volatile");
    result
}

#[inline(always)]
pub unsafe fn lidt(idtr: usize) {
    llvm_asm!("lidt (%rax)"::"{rax}"(idtr));
}

#[inline(always)]
pub unsafe fn rdmsr(ecx: u32) -> u64 {
    let edx: u32;
    let eax: u32;
    llvm_asm!("rdmsr":"={edx}"(edx), "={eax}"(eax):"{ecx}"(ecx));
    (edx as u64) << 32 | eax as u64
}

#[inline(always)]
pub unsafe fn wrmsr(ecx: u32, data: u64) {
    let edx: u32 = (data >> 32) as u32;
    let eax: u32 = data as u32;
    llvm_asm!("wrmsr"::"{edx}"(edx), "{eax}"(eax),"{ecx}"(ecx));
}

#[inline(always)]
pub unsafe fn cpuid(eax: &mut u32, ebx: &mut u32, ecx: &mut u32, edx: &mut u32) {
    llvm_asm!("cpuid":"={eax}"(*eax), "={ebx}"(*ebx), "={ecx}"(*ecx), "={edx}"(*edx):
        "{eax}"(eax.clone()), "{ecx}"(ecx.clone()));
}

#[inline(always)]
pub unsafe fn set_cr3(addr: usize) {
    llvm_asm!("movq %rax,%cr3"::"{rax}"(addr));
}

#[inline(always)]
pub unsafe fn invlpg(addr: usize) {
    llvm_asm!("invlpg (%rax)"::"{rax}"(addr));
}

pub unsafe fn clear_task_stack(
    task_switch_stack: usize,
    stack_size: usize,
    ss: u16,
    cs: u16,
    normal_stack_pointer: usize,
    start_addr: usize,
) {
    llvm_asm!("
                push    rdi
                mov     rdi, rsp
                mov     rsp, rax
                push    0
                push    0
                push    0
                push    0
                push    0
                push    0
                push    0
                push    0
                push    0
                push    0
                push    0
                push    0
                push    0
                push    rcx
                push    rbx
                pushfq
                pop     rax
                and     ax, 0x022a
                or      ax, 0x0200
                push    rax
                push    rsi
                push    rdx
                mov     rsp, rdi
                pop     rdi
    "::"{rax}"(task_switch_stack + stack_size), "{rbx}"(normal_stack_pointer), "{rcx}"(ss), "{rsi}"(cs), "{rdx}"(start_addr)::"intel", "volatile");
}

#[naked]
pub unsafe fn task_switch(
    now_context_data_address: *mut ContextData,
    next_context_data_address: *mut ContextData,
) {
    llvm_asm!("
                fxsave  [rsi]
                mov     [rsi + 512],          rax
                mov     [rsi + 512 + 8 *  1], rdx
                mov     [rsi + 512 + 8 *  2]. rcx
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
                mov     rax, fs
                mov     [rsi + 512 + 8 * 15], rax
                mov     rax, gs
                mov     [rsi + 512 + 8 * 16], rax
                mov     rax, ss
                mov     [rsi + 512 + 8 * 17], rax
                mov     [rsi + 512 + 8 * 18], rsp
                pushfq
                pop     rax
                and     ax, 0x022a
                or      rax, 0x200
                mov     [rsi + 512 + 8 * 19], rax   ; RFLAGS
                mov     rax, cs
                mov     [rsi + 512 + 8 * 20], rax
                mov     [rsi + 512 + 8 * 21], [rsp] ; RIP

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
                mov     fs, ax
                mov     rax, [rdi + 512 + 8 * 16]
                mov     gs, ax
                mov     rax, [rdi + 512]
                
                push    [rdi + 512 + 8 * 17] ; SS
                push    [rdi + 512 + 8 * 18] ; RSP
                push    [rdi + 512 + 8 * 19] ; RFLAGS
                push    [rdi + 512 + 8 * 20] ; CS
                push    [rdi + 512 + 8 * 21] ; RIP

                mov     rdi, [rdi + 512 + 8 *  6]
                iretq
                "::"{rsi}"(now_context_data_address),"{rdi}"(next_context_data_address)::"intel", "volatile");
}
