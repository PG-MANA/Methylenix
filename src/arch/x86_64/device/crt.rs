// CRT Controller

// use(Arch依存)
use super::cpu;

const CRT_CONTROLLER: u16 = 0x03d4;

// Grubがある程度の設定をしてることを利用して結構な設定を省略している
pub fn set_cursor_position(pos: u16) {
    unsafe {
        cpu::out_byte(CRT_CONTROLLER, 0x0e);
        cpu::out_byte(CRT_CONTROLLER + 1, ((pos & 0xff00) >> 0x0f) as u8);
        cpu::out_byte(CRT_CONTROLLER, 0x0f);
        cpu::out_byte(CRT_CONTROLLER + 1, (pos & 0xff) as u8);
    }
}
