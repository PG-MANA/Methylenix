/*
 * TTY Manager
 */

use kernel::sync::spin_lock::SpinLockFlag;

use core::fmt;

pub struct TtyManager {
    lock: SpinLockFlag,
    text_buffer: usize,
    input_queue: usize,
    output_queue: usize,
    driver: usize,
}

impl TtyManager {
    pub const fn new() -> Self {
        Self {
            lock: SpinLockFlag::new(),
            text_buffer: 0,
            input_queue: 0,
            output_queue: 0,
            driver: 0,
        }
    }

    pub fn open(&mut self, driver: usize) -> bool {
        unimplemented!()
    }

    pub fn puts(&mut self, s: &str) -> fmt::Result {
        let lock = if let Ok(l) = self.lock.try_lock() {
            l
        } else {
            return Err(fmt::Error {});
        };

        Ok(())
    }
}
