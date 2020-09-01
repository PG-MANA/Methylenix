//!
//! Interrupt Descriptor Table
//!
//! This module treats GateDescriptor.
//! This is usually used by InterruptManager.

/// GateDescriptor
///
/// This structure is from Intel Software Developer's Manual.
/// If you want the detail, please read "Intel SDM 6.14 EXCEPTION AND INTERRUPT HANDLING IN 64-BIT MODE".
///
/// ## Structure
///
///  * offset: the virtual address to call when interruption was happened
///  * selector: th segment selector which is used when switch to the handler
///  * ist: interrupt stack table: if it is not zero,
///         CPU will change the stack from the specific stack pointer of TSS
///  * type_attr: GateDescriptor's type(task gate, interrupt gate, and call gate) and privilege level
#[repr(C)]
pub struct GateDescriptor {
    offset_low: u16,
    selector: u16,
    ist: u8,
    type_attr: u8,
    offset_middle: u16,
    offset_high: u32,
    reserved: u32,
}

#[repr(C, packed)]
pub struct IDTR {
    pub limit: u16,
    pub offset: u64,
}

impl GateDescriptor {
    /// Create Gate Descriptor
    ///
    /// the detail is above.
    pub fn new(offset: unsafe fn(), selector: u16, ist: u8, type_attr: u8) -> GateDescriptor {
        let c = offset as *const unsafe fn() as usize;
        GateDescriptor {
            offset_low: (c & 0xffff) as u16,
            offset_middle: ((c & 0xffff0000) >> 16) as u16,
            offset_high: (c >> 32) as u32,
            selector,
            ist: ist & 0x07,
            type_attr,
            reserved: 0,
        }
    }
}
