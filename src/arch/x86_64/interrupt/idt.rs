/*
 * Copyright 2017 PG_MANA
 *
 * This software is Licensed under the Apache License Version 2.0
 * See LICENSE.md
 *
 * IDT(64bit)実装、割り込みを扱うために行う。
 */

//use
use arch::x86_64::device::cpu;

#[repr(C)]
pub struct GateDescriptor {
    offset_l: u16, //Offsetの下(0~15) //offset=ハンドラーの位置
    selector: u16, //セグメントセレクター
    ist: u8,       //TSSにあるスタック指定
    type_attr: u8, //IA32と同じ
    offset_m: u16, //Offsetの真ん中(16~31)
    offset_h: u32, //Offsetの上
    reserved: u32, //予約
}

#[repr(C, packed)]
pub struct IDTR {
    pub limit: u16,
    pub offset: u64,
}

//http://wiki.osdev.org/Interrupt_Descriptor_Table
impl GateDescriptor {
    pub const AR_INTGATE32: u8 = 0x008e & 0xff; //はりぼてOSより
    pub fn new(offset: unsafe fn(), selector: u16, ist: u8, type_attr: u8) -> GateDescriptor {
        //これを作るのは無害
        let c: usize = unsafe { cpu::get_func_addr(offset) }; //ここだけ不安定
        GateDescriptor {
            offset_l: (c & 0xffff) as u16,             //(offset & 0xffff) as u16,
            offset_m: ((c & 0xffff0000) >> 16) as u16, //(offset & 0xffff0000 >> 16) as u16,
            offset_h: (c >> 32) as u32,                //(offset >> 32) as u32,
            selector: selector,
            ist: ist & 0x07,
            type_attr: type_attr,
            reserved: 0,
        }
    }
}
