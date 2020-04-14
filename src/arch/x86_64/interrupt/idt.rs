/*
 * Interrupt Descriptor Table
 */

#[repr(C)]
pub struct GateDescriptor {
    offset_l: u16,
    //Offsetの下(0~15) //offset=ハンドラーの位置
    selector: u16,
    //セグメントセレクター
    ist: u8,
    //TSSにあるスタック指定(Interrupt Stack Table)
    type_attr: u8,
    offset_m: u16,
    //Offsetの真ん中(16~31)
    offset_h: u32,
    //Offsetの上
    reserved: u32,
}

#[repr(C, packed)]
pub struct IDTR {
    pub limit: u16,
    pub offset: u64,
}

/* Intel SDM 6.14 EXCEPTION AND INTERRUPT HANDLING IN 64-BIT MODE */
impl GateDescriptor {
    pub fn new(offset: unsafe fn(), selector: u16, ist: u8, type_attr: u8) -> GateDescriptor {
        let c = offset as *const unsafe fn() as usize;
        GateDescriptor {
            offset_l: (c & 0xffff) as u16,             //(offset & 0xffff) as u16,
            offset_m: ((c & 0xffff0000) >> 16) as u16, //(offset & 0xffff0000 >> 16) as u16,
            offset_h: (c >> 32) as u32,                //(offset >> 32) as u32,
            selector,
            ist: ist & 0x07,
            type_attr,
            reserved: 0,
        }
    }
}
