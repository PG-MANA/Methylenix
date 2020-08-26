//!
//! 8259 PIC
//!
//! Use APIC instead.
//!

use arch::target_arch::device::cpu;

const PIC0_IMR: u16 = 0x0021;
const PIC1_IMR: u16 = 0x00a1;

/// Disable 8259 PIC
///
/// This function sets all interruption from PIC disallowed.
pub fn disable_8259_pic() {
    unsafe {
        cpu::out_byte(PIC0_IMR, 0xff); /* Disallow all interrupt */
        cpu::out_byte(PIC1_IMR, 0xff); /* Disallow all interrupt */
    }
}
