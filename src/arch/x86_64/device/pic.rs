/*
PIC初期化
参考:https://pdos.csail.mit.edu/6.828/2005/readings/hardware/8259A.pdf
*/

use arch::target_arch::device::cpu;

//const PIC0_ISR: u16 = 0x0020;
const PIC0_ICW1: u16 = 0x0020;
const PIC0_OCW2: u16 = 0x0020;
const PIC0_OCW3: u16 = 0x0020;
const PIC0_IMR: u16 = 0x0021;
const PIC0_ICW2: u16 = 0x0021;
const PIC0_ICW3: u16 = 0x0021;
const PIC0_ICW4: u16 = 0x0021;
//const PIC1_ISR: u16 = 0x00a0;
const PIC1_ICW1: u16 = 0x00a0;
//const PIC1_OCW2: u16 = 0x00a0;
const PIC1_OCW3: u16 = 0x00a0;
const PIC1_IMR: u16 = 0x00a1;
const PIC1_ICW2: u16 = 0x00a1;
const PIC1_ICW3: u16 = 0x00a1;
const PIC1_ICW4: u16 = 0x00a1;

pub unsafe fn pic_init() {
    cpu::out_byte(PIC0_IMR, 0xff); /* 全ての割り込みを受け付けない */
    cpu::out_byte(PIC1_IMR, 0xff); /* 全ての割り込みを受け付けない */

    cpu::out_byte(PIC0_ICW1, 0x11); /* エッジトリガモード */
    cpu::out_byte(PIC0_ICW2, 0x20); /* IRQ0-7は、INT20-27で受ける */
    cpu::out_byte(PIC0_ICW3, 1 << 2); /* PIC1はIRQ2にて接続 */
    cpu::out_byte(PIC0_ICW4, 0x01); /* ノンバッファモード */
    cpu::out_byte(PIC0_OCW3, 0b1011u8); /*IRR/ISR読み出しポートの設定:ISRに設定 */

    cpu::out_byte(PIC1_ICW1, 0x11); /* エッジトリガモード */
    cpu::out_byte(PIC1_ICW2, 0x28); /* IRQ8-15は、INT28-2fで受ける */
    cpu::out_byte(PIC1_ICW3, 0x02); /* PIC1はIRQ2にて接続 */
    cpu::out_byte(PIC1_ICW4, 0x01); /* ノンバッファモード */
    cpu::out_byte(PIC1_OCW3, 0b1011u8); /*IRR/ISR読み出しポートの設定:ISRに設定 */

    cpu::out_byte(PIC0_IMR, 0xfb); /* 11111011 PIC1以外は全て禁止 */
    cpu::out_byte(PIC1_IMR, 0xff); /* 11111111 全ての割り込みを受け付けない */
}

pub unsafe fn pic0_accept(bit: u8) {
    /*許可したいbitを立てて渡す*/
    cpu::out_byte(PIC0_IMR, !bit & cpu::in_byte(PIC0_IMR));
}
/*
pub unsafe fn pic1_accept(bit :  u8) {/*許可したいbitを立てて渡す*/
    cpu::out_byte(PIC1_IMR, !bit &  cpu::in_byte(PIC1_IMR));
}
*/
pub unsafe fn pic0_eoi(irq: u8) {
    cpu::out_byte(PIC0_OCW2, irq + 0x60);
}
/*
pub unsafe fn pic1_eoi(irq : u8) {
    cpu::out_byte(PIC1_OCW2,irq + 0x60);
}

pub unsafe fn get_isr_master() -> u8 {
    cpu::in_byte(PIC0_ISR)
}

pub unsafe fn get_isr_slave() -> u8 {
    cpu::in_byte(PIC1_ISR)
}
*/
