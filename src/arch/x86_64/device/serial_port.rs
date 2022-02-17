//!
//! Serial Port Manager
//!
//! This manages general serial communication.

use crate::arch::target_arch::device::cpu::{in_byte, out_byte};

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::sync::spin_lock::SpinLockFlag;

/// SerialPortManager
///
/// SerialPortManager has SpinLockFlag inner.
/// Default Fifo size is 256 byte. In the future, it may variable by using vec<u8>
pub struct SerialPortManager {
    port: u16,
    write_lock: SpinLockFlag,
    enabled: bool,
}

impl SerialPortManager {
    /// Create SerialPortManager with io_port.
    ///
    /// Only send data by serial port, it is needless to call [`init`].
    /// If you want to enable interruption of arriving data, you should call [`init`].
    ///
    /// [`init`]: #method.init
    pub fn new(io_port: u16) -> SerialPortManager {
        Self {
            port: io_port,
            write_lock: SpinLockFlag::new(),
            enabled: true,
        }
    }

    /// Get using io port.
    pub fn get_port(&self) -> u16 {
        self.port
    }

    /// Setup interruption.
    ///
    /// This function makes interrupt handler and registers it to InterruptManager.
    /// After registering, send the controller to allow IRQ interruption.  
    pub fn init(&self) {
        let _ = get_kernel_manager_cluster()
            .boot_strap_cpu_manager
            .interrupt_manager
            .set_device_interrupt_function(Self::int_handler24_main, Some(4), None, 0, false);
        let _lock = self.write_lock.lock();
        unsafe {
            out_byte(self.port + 1, 0x00); // Off the FIFO of controller
            out_byte(self.port + 3, 0x80); // Enable DLAB
                                           //out_byte(self.port + 0, 0x03); // Set lower of the rate
                                           //out_byte(self.port + 1, 0x00); // Set higher of the rate
            out_byte(self.port + 3, 0x03); // Set the data style: 8bit no parity bit
            out_byte(self.port + 1, 0x05); // Fire an interruption on new data or error
            out_byte(self.port + 2, 0xC7); // On FIFO and allow interruption
            out_byte(self.port + 4, 0x0B); // Start IRQ interruption
        }
    }

    /// Send a 8bit data.
    ///
    /// If serial port is full or unusable, this function tries 0xFF times and fallback.
    pub fn send(&mut self, data: u8) {
        if self.port == 0 || !self.enabled {
            return;
        }
        let _lock = self.write_lock.lock();
        self._send(data);
    }

    fn _send(&mut self, data: u8) {
        let mut timeout: usize = 0xFFFF;
        while timeout > 0 {
            if self.is_completed_transmitter() {
                break;
            }
            timeout -= 1;
        }
        if timeout == 0 {
            self.enabled = false;
            return;
        }
        unsafe {
            out_byte(self.port, data);
        }
    }

    /// Send a string.
    ///
    /// This function sends str by calling [`send`] by each bytes.
    /// If serial port is full or unusable, this function **may take long time**.
    ///
    /// [`send`]: #method.send
    pub fn send_str(&mut self, s: &str) {
        if self.port == 0 || !self.enabled {
            return;
        }
        let _lock = self.write_lock.lock();
        for c in s.bytes() {
            if c as char == '\n' {
                self._send('\r' as u8);
            }
            self._send(c);
        }
    }

    /// Read a 8bit-data from serial port.
    ///
    /// Read an u8-data from the controller with io port.
    /// This function is used to enqueue the data into FIFO.
    fn read(&self) -> u8 {
        if self.port == 0 {
            return 0;
        }
        unsafe { in_byte(self.port) }
    }

    /// Serial Port interrupt handler
    ///
    /// First, this will get data from serial port controller, and push it into FIFO.
    /// Currently, this wakes the main process up.
    fn int_handler24_main(_: usize) -> bool {
        get_kernel_manager_cluster()
            .kernel_tty_manager
            .input_from_interrupt_handler(get_kernel_manager_cluster().serial_port_manager.read());
        return true;
    }

    /// Check if the transmission was completed.
    #[inline]
    fn is_completed_transmitter(&self) -> bool {
        (unsafe { in_byte(self.port + 5) } & 0x40) != 0
    }
}
