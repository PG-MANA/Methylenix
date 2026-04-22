//!
//! Print to Serial Port
//!

use crate::arch::target_arch::device::cpu::flush_data_cache_all;

use core::fmt;

pub(super) struct SerialPort {
    base_address: *mut u32,
    wait_address: *mut u32,
    wait_value: u32,
}

pub(super) static mut WRITER: Option<SerialPort> = None;

impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        use core::ptr::{read_volatile, write_volatile};
        fn wait_fifo(address: *mut u32, value: u32) {
            if !address.is_null() && value != 0 {
                for _ in 0..u16::MAX {
                    if (unsafe { read_volatile(address) } & value) != 0 {
                        break;
                    }
                }
            }
        }

        for c in s.as_bytes() {
            if *c == b'\n' {
                wait_fifo(self.wait_address, self.wait_value);
                unsafe { write_volatile(self.base_address, b'\r' as _) };
                flush_data_cache_all();
            }
            wait_fifo(self.wait_address, self.wait_value);
            unsafe { write_volatile(self.base_address, *c as _) };

            flush_data_cache_all();
        }
        Ok(())
    }
}

pub fn init(uart_address: usize, wait_offset: Option<u32>, wait_value: Option<u32>) {
    unsafe {
        *(&raw mut WRITER).as_mut().unwrap() = Some(SerialPort {
            base_address: uart_address as *mut _,
            wait_address: wait_offset
                .map(|o| (uart_address + o as usize) as *mut u32)
                .unwrap_or(core::ptr::null_mut()),
            wait_value: wait_value.unwrap_or(0),
        })
    };
}
