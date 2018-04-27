/*
 * Copyright 2017 PG_MANA
 *
 * This software is Licensed under the Apache License Version 2.0
 * See LICENSE.md
 *
 * x86_64に関するCPU命令
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

pub unsafe fn lidt(idtr: usize) {
    asm!("lidt (%rax)"::"{rax}"(idtr));
}

pub unsafe fn get_func_addr(func: unsafe fn()) -> usize {
    // 関数のアドレス取得に使用、代用案捜索中
    #[allow(unused_assignments)]
    let mut result: usize = 0;
    asm!("mov eax, ebx ":"={eax}"(result):"{ebx}"(func)::"intel");
    result
}
