/*
 * 8259 PIC
 * use APIC instead.
 */

use arch::target_arch::device::cpu;

const PIC0_IMR: u16 = 0x0021;
const PIC1_IMR: u16 = 0x00a1;

pub fn disable_8259_pic() {
    unsafe {
        cpu::out_byte(PIC0_IMR, 0xff); /* Disallow all interrupt */
        cpu::out_byte(PIC1_IMR, 0xff); /* Disallow all interrupt */
    }
}
