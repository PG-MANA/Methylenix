/*
 * Copyright 2017 PG_MANA
 *
 * This software is Licensed under the Apache License Version 2.0
 * See LICENSE.md
 *
 * TSS(64bit)実装、割り込みを扱うために行う。
 */

use core::mem;
const IO_MAP_SIZE: usize = 0xFFFF;

#[repr(C)]
pub struct TSS {
    reserved_1: u32,
    rsp0_l: u32,
    rsp0_u: u32,
    rsp1_l: u32,
    rsp1_u: u32,
    rsp2_l: u32,
    rsp2_u: u32,
    reserved_2: u32,
    reserved_3: u32,
    ist_0_l: u32,
    ist_0_u: u32,
    ist_1_l: u32,
    ist_1_u: u32,
    ist_2_l: u32,
    ist_2_u: u32,
    ist_3_l: u32,
    ist_3_u: u32,
    ist_4_l: u32,
    ist_4_u: u32,
    ist_5_l: u32,
    ist_5_u: u32,
    ist_6_l: u32,
    ist_6_u: u32,
    ist_7_l: u32,
    ist_7_u: u32,
    reserved_4: u32,
    reserved_5: u32,
    res_and_iomap: u32,
    //I/O Mapにアクセスできるかの権限を表すメモリ空間へのポインタ(0:許可,1:禁止)
    io_permission_map: [u8; IO_MAP_SIZE / 8],
}

impl TSS {
    pub fn new(rsp0: u64) -> TSS {
        let mut res: TSS = unsafe { mem::zeroed() };
        res.rsp0_l = (rsp0 & 0xffffffff) as u32;
        res.rsp0_u = (rsp0 & 0xffffffff00000000 >> 32) as u32;
        res.res_and_iomap = (mem::size_of::<TSS>() << 16) as u32;
        res
    }
}