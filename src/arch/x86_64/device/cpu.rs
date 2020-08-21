/*
 * x86_64 Specific Instruction
 */

use arch::target_arch::context::context_data::ContextData;

#[inline(always)]
pub unsafe fn sti() {
    llvm_asm!("sti");
}

#[inline(always)]
pub unsafe fn cli() {
    llvm_asm!("cli");
}

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
    llvm_asm!("outb %al, %dx"::"{dx}"(addr), "{al}"(data)::"volatile");
}

#[inline(always)]
pub unsafe fn in_byte(port: u16) -> u8 {
    let result: u8;
    llvm_asm!("in %dx, %al":"={al}"(result):"{dx}"(port)::"volatile");
    result
}

#[inline(always)]
pub unsafe fn in_byte_twice(port: u16) -> (u8 /*first*/, u8 /*second*/) {
    let r1: u8;
    let r2: u8;
    llvm_asm!("in  %dx, %al
               mov %al, %bl
               in  %dx, %al":"={bl}"(r1),"={al}"(r2):"{dx}"(port)::"volatile");
    (r1, r2)
}

#[inline(always)]
pub unsafe fn in_dword(port: u16) -> u32 {
    let result: u32;
    llvm_asm!("in %dx, %eax":"={eax}"(result):"{dx}"(port)::"volatile");
    result
}

#[inline(always)]
pub unsafe fn lidt(idtr: usize) {
    llvm_asm!("lidt (%rdi)"::"{rdi}"(idtr));
}

#[inline(always)]
pub unsafe fn rdmsr(ecx: u32) -> u64 {
    let edx: u32;
    let eax: u32;
    llvm_asm!("rdmsr":"={edx}"(edx), "={eax}"(eax):"{ecx}"(ecx)::"volatile");
    (edx as u64) << 32 | eax as u64
}

#[inline(always)]
pub unsafe fn wrmsr(ecx: u32, data: u64) {
    let edx: u32 = (data >> 32) as u32;
    let eax: u32 = data as u32;
    llvm_asm!("wrmsr"::"{edx}"(edx), "{eax}"(eax),"{ecx}"(ecx)::"volatile");
}

#[inline(always)]
pub unsafe fn cpuid(eax: &mut u32, ebx: &mut u32, ecx: &mut u32, edx: &mut u32) {
    llvm_asm!("cpuid":"={eax}"(*eax), "={ebx}"(*ebx), "={ecx}"(*ecx), "={edx}"(*edx):
        "{eax}"(eax.clone()), "{ecx}"(ecx.clone()));
}

#[inline(always)]
pub unsafe fn get_cr0() -> u64 {
    let rax: u64;
    llvm_asm!("movq %cr0, %rax":"={rax}"(rax):::"volatile");
    rax
}

#[inline(always)]
pub unsafe fn set_cr0(cr4: u64) {
    llvm_asm!("movq %rdi, %cr0"::"{rdi}"(cr4)::"volatile");
}

#[inline(always)]
pub unsafe fn set_cr3(addr: usize) {
    llvm_asm!("movq %rdi, %cr3"::"{rdi}"(addr)::"volatile");
}

#[inline(always)]
pub unsafe fn get_cr3() -> usize {
    let mut rax: u64;
    llvm_asm!("movq %cr3, %rax":"={rax}"(rax):::"volatile");
    rax as usize
}

#[inline(always)]
pub unsafe fn get_cr4() -> u64 {
    let mut rax: u64;
    llvm_asm!("movq %cr4, %rax":"={rax}"(rax):::"volatile");
    rax
}

#[inline(always)]
pub unsafe fn set_cr4(cr4: u64) {
    llvm_asm!("movq %rdi, %cr4"::"{rdi}"(cr4)::"volatile");
}

#[inline(always)]
pub unsafe fn invlpg(addr: usize) {
    llvm_asm!("invlpg (%rdi)"::"{rdi}"(addr):::"volatile");
}

#[naked]
pub unsafe fn run_task(context_data_address: *mut ContextData) {
    llvm_asm!("

                mov     rax, [rdi + 512 + 8 * 18]
                add     rax, 8                      // Stack-Alignment(is it OK?)
                mov     [rdi + 512 + 8 * 18], rax

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
                
                push    [rdi + 512 + 8 * 17] // SS
                push    [rdi + 512 + 8 * 18] // RSP
                push    [rdi + 512 + 8 * 19] // RFLAGS
                push    [rdi + 512 + 8 * 20] // CS
                push    [rdi + 512 + 8 * 21] // RIP

                mov     rax, [rdi + 512 + 8 * 22]
                mov     cr3, rax
                mov     rax, [rdi + 512]
                mov     rdi, [rdi + 512 + 8 *  6]
                iretq
                "::"{rdi}"(context_data_address)::"intel", "volatile");
}

#[naked]
pub unsafe fn task_switch(
    next_context_data_address: *mut ContextData,
    now_context_data_address: *mut ContextData,
) {
    llvm_asm!("
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
                mov     rax, fs
                mov     [rsi + 512 + 8 * 15], rax
                mov     rax, gs
                mov     [rsi + 512 + 8 * 16], rax
                mov     rax, ss
                mov     [rsi + 512 + 8 * 17], rax
                mov     rax, rsp
                mov     [rsi + 512 + 8 * 18], rax
                pushfq
                pop     rax
                and     ax, 0x022a
                or      rax, 0x200
                mov     [rsi + 512 + 8 * 19], rax   // RFLAGS
                mov     rax, cs
                mov     [rsi + 512 + 8 * 20], rax
                lea     rax, 1f
                mov     [rsi + 512 + 8 * 21], rax   // RIP
                mov     rax, cr3
                mov     [rsi + 512 + 8 * 22], rax

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
                
                push    [rdi + 512 + 8 * 17] // SS
                push    [rdi + 512 + 8 * 18] // RSP
                push    [rdi + 512 + 8 * 19] // RFLAGS
                push    [rdi + 512 + 8 * 20] // CS
                push    [rdi + 512 + 8 * 21] // RIP

                mov     rax, [rdi + 512 + 8 * 22]
                mov     cr3, rax
                mov     rax, [rdi + 512]
                mov     rdi, [rdi + 512 + 8 *  6]
                iretq
                1:
                "::"{rsi}"(now_context_data_address),"{rdi}"(next_context_data_address)::"intel", "volatile");
}
