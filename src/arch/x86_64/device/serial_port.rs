/*
 * Serial Port Manager
 */

use arch::target_arch::device::cpu::{in_byte, out_byte};

use kernel::fifo::FIFO;
use kernel::manager_cluster::get_kernel_manager_cluster;
use kernel::sync::spin_lock::SpinLockFlag;

pub struct SerialPortManager {
    port: u16,
    write_lock: SpinLockFlag,
    fifo: FIFO<u8, 256usize>,
}

impl SerialPortManager {
    pub const fn new(io_port: u16) -> SerialPortManager {
        SerialPortManager {
            port: io_port,
            write_lock: SpinLockFlag::new(),
            fifo: FIFO::new(&0),
        }
    }

    pub fn get_port(&self) -> u16 {
        self.port //あとから変更できないようにする
    }

    pub fn init(&self) {
        unsafe {
            make_interrupt_hundler!(inthandler24, SerialPortManager::inthandler24_main);
            get_kernel_manager_cluster()
                .interrupt_manager
                .lock()
                .unwrap()
                .set_device_interrupt_function(
                    inthandler24, /*上のマクロで指定した名前*/
                    4,
                    0x24,
                    0,
                );
            let _lock = self.write_lock.lock();
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
        let _lock = self.write_lock.lock();
        let mut timeout: usize = 0xFF;
        while timeout > 0 {
            if self.is_completed_transmitter() {
                break;
            }
            timeout -= 1;
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

    pub fn enqueue_key(&mut self, key: u8) {
        self.fifo.enqueue(key);
    }

    pub fn inthandler24_main() {
        //handlerをimplで実装することを考え直すべき
        let m = &mut get_kernel_manager_cluster().serial_port_manager;
        m.enqueue_key(m.read());
        if let Ok(interrupt_manager) = get_kernel_manager_cluster().interrupt_manager.try_lock() {
            interrupt_manager.send_eoi();
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
