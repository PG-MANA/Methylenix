// EFIブートでテキストフレームバッファが使えないので
// TODO: UEFIでシリアルポート

//use
use arch::target_arch::device::cpu::{in_byte, out_byte};
use arch::target_arch::device::local_apic;
use arch::target_arch::interrupt::idt::GateDescriptor;

use kernel::fifo::FIFO;
use kernel::struct_manager::STATIC_BOOT_INFORMATION_MANAGER;


pub struct SerialPortManager {
    port: u16,
    fifo: FIFO<u8>,
}

impl SerialPortManager {
    pub fn new(io_port: u16) -> SerialPortManager {
        SerialPortManager {
            port: io_port,
            fifo: FIFO::new(128),
        }
    }

    pub const fn new_static() -> SerialPortManager {
        SerialPortManager {
            port: 0x3F8,
            fifo: FIFO::new_static(128, &0),
        }
    }

    pub fn get_port(&self) -> u16 {
        self.port //あとから変更できないようにする
    }

    pub fn init(&self, selector: u16) {
        unsafe {
            make_interrupt_hundler!(inthandler24, SerialPortManager::inthandler24_main);
            STATIC_BOOT_INFORMATION_MANAGER.interrupt_manager.lock().unwrap().set_gatedec(
                0x24,
                GateDescriptor::new(
                    inthandler24, /*上のマクロで指定した名前*/
                    selector,
                    0,
                    GateDescriptor::AR_INTGATE32,
                ),
            );

            out_byte(self.port + 1, 0x00); // FIFOをオフ
            out_byte(self.port + 3, 0x80); // DLABを有効化して設定できるようにする?
            //out_byte(self.port + 0, 0x03); // rateを設定
            //out_byte(self.port + 1, 0x00); // rate上位
            out_byte(self.port + 3, 0x03); // 8bit単位のパリティビットなし
            out_byte(self.port + 1, 0x05); // データ到着とエラーで割り込み
            out_byte(self.port + 2, 0xC7); // FIFOをオン、割り込みを許可
            out_byte(self.port + 4, 0x0B); // IRQ割り込みを開始
        }
    }

    pub fn send(&self, data: u8) {
        if self.port == 0 {
            return;
        }
        loop {
            if self.is_completed_transmitter() {
                break;
            }
        } // ちょっと危なっかしい
        unsafe {
            out_byte(self.port, data);
        }
    }

    pub fn sendstr(&self, s: &str) {
        for c in s.bytes() {
            if c as char == '\n' {
                self.send('\r' as u8);
            }
            self.send(c);
        }
    }

    pub fn read(&self) -> u8 {
        if self.port == 0 {
            return 0;
        }
        unsafe { in_byte(self.port) }
    }

    pub fn dequeue_key(&mut self) -> Option<u8> {
        self.fifo.dequeue()
    }

    pub fn inthandler24_main() {
        //handlerをimplで実装することを考え直すべき
        unsafe {
            if let Ok(mut serial_port_manager) = STATIC_BOOT_INFORMATION_MANAGER.serial_port_manager.try_lock() {
                let code = serial_port_manager.read();
                serial_port_manager.fifo.queue(code);
            }
            local_apic::LocalApicManager::send_eoi();
        }
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
