// EFIブートでテキストフレームバッファが使えないので
// TODO: UEFIでシリアルポート
// TODO: 割り込みハンドラ

//use
use super::cpu::{in_byte, out_byte};

pub struct SerialPortManager {
    port: u16,
}

impl SerialPortManager {
    pub fn new(port: u16) -> SerialPortManager {
        SerialPortManager { port: port }
    }

    pub const fn new_static() -> SerialPortManager {
        SerialPortManager { port: 0 }
    }

    pub fn get_port(&self) -> u16 {
        self.port //あとから変更できないようにする
    }

    pub fn init_serial_port(&self) {
        unsafe {
            out_byte(self.port + 1, 0x00); // FIFOをオフ
            out_byte(self.port + 3, 0x80); // DLABを有効化して設定できるようにする?
            out_byte(self.port + 0, 0x03); // rateを設定
            out_byte(self.port + 1, 0x00); // rate上位
            out_byte(self.port + 3, 0x03); // 8bit単位のパリティビットなし
            out_byte(self.port + 1, 0x05); // データ到着とエラーで割り込み
            out_byte(self.port + 2, 0xC7); // FIFOをオン、割り込みを許可
        }
    }

    pub fn send(&self, data: u8) {
        loop {
            if self.is_completed_transmitter() {
                break;
            }
        } // ちょっと危なっかしい
        unsafe {
            out_byte(self.port, data);
        }
    }

    pub fn read(&self) -> u8 {
        unsafe { in_byte(self.port) }
    }
    fn is_completed_transmitter(&self) -> bool {
        unsafe {
            if in_byte(self.port + 5) & 0x40 != 0 {
                true
            } else {
                false
            }
        }
    }
}
