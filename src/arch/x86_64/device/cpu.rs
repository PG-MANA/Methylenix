/*
x86_64に関するCPU命令
*/

#[inline(always)]
pub unsafe fn sti() {
    asm!("sti");
}
/*
#[inline(always)]
pub unsafe fn cli() {
    asm!("cli");
}
*/
#[inline(always)]
pub unsafe fn hlt() {
    asm!("hlt");
}

#[inline(always)]
pub unsafe fn out_byte(addr: u16, data: u8) {
    asm!("outb %al, %dx"::"{dx}"(addr), "{al}"(data));
}

#[inline(always)]
pub unsafe fn in_byte(data: u16) -> u8 {
    let result: u8;
    asm!("in %dx, %al":"={al}"(result):"{dx}"(data)::"volatile");
    result
}

#[inline(always)]
pub unsafe fn lidt(idtr: usize) {
    asm!("lidt (%rax)"::"{rax}"(idtr));
}

#[inline(always)]
pub unsafe fn rdmsr(ecx: u32) -> u64 {
    let edx: u32;
    let eax: u32;
    asm!("rdmsr":"={edx}"(edx), "={eax}"(eax):"{ecx}"(ecx));
    (edx as u64) << 32 | eax as u64
}

#[inline(always)]
pub unsafe fn wrmsr(ecx: u32, data: u64) {
    let edx: u32 = (data >> 32) as u32;
    let eax: u32 = data as u32;
    asm!("wrmsr"::"{edx}"(edx), "{eax}"(eax),"{ecx}"(ecx));
}

#[inline(always)]
pub unsafe fn cpuid(eax: &mut u32, ebx: &mut u32, ecx: &mut u32, edx: &mut u32) {
    asm!("cpuid":"={eax}"(*eax), "={ebx}"(*ebx), "={ecx}"(*ecx), "={edx}"(*edx):
        "{eax}"(eax.clone()), "{ecx}"(ecx.clone()));
}

#[inline(always)]
pub unsafe fn set_cr3(addr: usize) {
    asm!("movq %rax,%cr3"::"{rax}"(addr));
}

#[inline(always)]
pub unsafe fn invlpg(addr: usize) {
    asm!("invlpg (%rax)"::"{rax}"(addr));
}

#[inline(always)]
pub unsafe fn get_func_addr(func: unsafe fn()) -> usize {
    // 関数のアドレス取得に使用、代用案捜索中
    #[allow(unused_assignments)]
        let mut result: usize = 0;
    asm!("mov rax, rbx":"={rax}"(result):"{rbx}"(func)::"intel");
    result
}

pub unsafe fn clear_task_stack(task_switch_stack: usize, stack_size: usize, ss: u16, cs: u16, normal_stack_pointer: usize, start_addr: usize) {
    asm!("
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
pub unsafe fn task_switch(now_task_stack: usize, next_task_stack: usize, stack_size: usize) {
    asm!("
                push    rdx
                mov     rdx, rsp
                mov     rsp, rax
                push    rdx
                push    rcx
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
                mov     rax, ss
                push    rax
                mov     rax, rsp
                add     rax, 8
                push    rax
                pushfq
                pop     rax
                and     ax, 0x022a
                or      rax, 0x200
                push    rax
                mov     rax, cs
                push    rax
                lea     rax, 1f
                push    rax
                mov     rsp, rbx
                iretq
           1:
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
                pop     rcx
                pop     rsp
                pop     rdx
                "::"{rax}"(now_task_stack + stack_size),"{rbx}"((next_task_stack + stack_size) - 18 * 8)::"intel", "volatile");
}