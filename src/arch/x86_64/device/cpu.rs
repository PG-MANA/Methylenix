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
    asm!("in %dx, %al":"={al}"(result):"{dx}"(data));
    result
}

#[inline(always)]
pub unsafe fn lidt(idtr: usize) {
    asm!("lidt (%rax)"::"{rax}"(idtr));
}

#[inline(always)]
pub unsafe fn set_cr3(addr: usize) {
    asm!("movq %rax,%cr3"::"{rax}"(addr));
}

#[inline(always)]
pub unsafe fn invlpg(addr: usize) {
    asm!("invlpg (%rax)"::"{rax}"(addr));
}


pub unsafe fn get_func_addr(func: unsafe fn()) -> usize {
    // 関数のアドレス取得に使用、代用案捜索中
    #[allow(unused_assignments)]
        let mut result: usize = 0;
    asm!("mov rax, rbx ":"={rax}"(result):"{rbx}"(func)::"intel");
    result
}

pub unsafe fn get_init_handler_func_addr(func: unsafe extern "x86-interrupt" fn()) -> usize {
    // 関数のアドレス取得に使用、代用案捜索中
    #[allow(unused_assignments)]
        let mut result: usize = 0;
    asm!("mov rax, rbx ":"={rax}"(result):"{rbx}"(func)::"intel");
    result
}

#[naked]
pub unsafe fn clear_task_stack(task_switch_stack: usize, stack_size: usize, privilege_level: u8, normal_stack_pointer: usize, start_addr: usize) {
    let cs: usize;
    asm!("mov   rax, cs":"={rax}"(cs):::"intel");
    asm!("
                push    rdi
                mov     rdi,rsp
                mov     rsp,rax
                push    rdx
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
                push    rbx
                push    rcx
                mov     rsp, rdi
                pop     rdi
    "::"{rax}"(task_switch_stack + stack_size),"{rbx}"((cs & 0xFFFC) | privilege_level as usize),"{rcx}"(start_addr),"[rdx}"(normal_stack_pointer)::"intel");
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
                push    0
                mov     rax, cs
                push    rax
                lea     rax, 1f
                push    rax
                mov     rsp, rbx
                sti
                retfq
           1:
                pop     r15
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
                "::"{rax}"(now_task_stack + stack_size),"{rbx}"((next_task_stack + stack_size) - 16 * 8)::"intel");
}