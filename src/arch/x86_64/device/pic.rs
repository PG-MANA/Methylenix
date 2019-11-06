/*
PIC
APICを使うので無効にするだけ
*/

use arch::target_arch::device::cpu;

const PIC0_IMR: u16 = 0x0021;
const PIC1_IMR: u16 = 0x00a1;

pub fn pic_init() {
    unsafe {
        cpu::out_byte(PIC0_IMR, 0xff); /* 全ての割り込みを受け付けない */
        cpu::out_byte(PIC1_IMR, 0xff); /* 全ての割り込みを受け付けない */
    }
}
