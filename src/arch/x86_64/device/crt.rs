//!
//! CRT Controller
//!
//! To control the position of cursor on the VGA text mode.
//!

use super::cpu;

const CRT_CONTROLLER: u16 = 0x03d4;

/// Set the cursor position of VGA text mode.
///
/// pos = y * text_buffer_width + x
pub fn set_cursor_position(pos: u16) {
    unsafe {
        cpu::out_byte(CRT_CONTROLLER, 0x0e);
        cpu::out_byte(CRT_CONTROLLER + 1, (pos >> 8) as u8);
        cpu::out_byte(CRT_CONTROLLER, 0x0f);
        cpu::out_byte(CRT_CONTROLLER + 1, (pos & 0xff) as u8);
    }
}
