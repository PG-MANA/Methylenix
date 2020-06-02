/*
 * CRT Controller
 */

use super::cpu;

const CRT_CONTROLLER: u16 = 0x03d4;

pub fn set_cursor_position(pos: u16) {
    unsafe {
        cpu::out_byte(CRT_CONTROLLER, 0x0e);
        cpu::out_byte(CRT_CONTROLLER + 1, (pos >> 8) as u8);
        cpu::out_byte(CRT_CONTROLLER, 0x0f);
        cpu::out_byte(CRT_CONTROLLER + 1, (pos & 0xff) as u8);
    }
}
