//!
//! Programmable Interval Timer 8254
//!
//! It is the legacy timer implemented 16bit programmable timer.
//! The frequency is 1.19318MHz.
//! This timer is used to sync Local APIC Timer.

use crate::arch::target_arch::device::cpu;

use crate::kernel::sync::spin_lock::SpinLockFlag;
use crate::kernel::timer_manager::Timer;

/// PitManager
///
/// PitManager has SpinLockFlag inner.
/// Reload value is 11932 and it is constant.
/// When the interrupt started, this timer will interrupt each 10ms.
/// If Local APIC is usable, it is better to use it.
pub struct PitManager {
    lock: SpinLockFlag,
    reload_value: u16,
}

impl PitManager {
    /// Create IoApicManager with invalid data.
    ///
    /// Before use, **you must call [`init`]**.
    ///
    /// [`init`]: #method.init
    pub const fn new() -> Self {
        Self {
            lock: SpinLockFlag::new(),
            reload_value: 0,
        }
    }

    /// Init PIT as reload_value is 0xffff.
    ///
    /// After calling this function, the count down of the PIT will start immediately.
    pub fn init(&mut self) {
        let _lock = self.lock.lock();
        unsafe {
            cpu::out_byte(0x43, 0x34);
            cpu::out_byte(0x40, 0xff);
            cpu::out_byte(0x40, 0xff);
        }
        self.reload_value = 0xffff;
    }

    /// Stop PIT.
    ///
    /// This function set reload_value to zero and PIT will stop counting.
    /// `get_count` will return zero and the timer interrupt will stop.
    pub fn stop_counting(&mut self) {
        let _lock = self.lock.lock();
        unsafe { cpu::out_byte(0x43, 0) };
        self.reload_value = 0;
    }
}

impl Timer for PitManager {
    #[inline(always)]
    fn get_count(&self) -> usize {
        /*let _lock = self.lock.lock();*/
        unsafe { cpu::out_byte(0x43, 0) };
        let (r1, r2) = unsafe { cpu::in_byte_twice(0x40) };
        ((r2 as usize) << 8) | r1 as usize
    }

    fn get_frequency_hz(&self) -> usize {
        1193182
    }

    fn is_count_up_timer(&self) -> bool {
        false
    }

    fn get_difference(
        &self,
        earlier: usize, /*earlier*/
        later: usize,   /*later*/
    ) -> usize {
        /*assume that counter is not rotated more than once.*/
        if earlier <= later {
            earlier + (self.reload_value as usize - later)
        } else {
            earlier - later
        }
    }

    fn get_ending_count_value(&self, start: usize, difference: usize) -> usize {
        if start > difference {
            start - difference
        } else {
            self.reload_value as usize - (difference - start)
        }
    }

    fn get_max_counter_value(&self) -> usize {
        self.reload_value as usize
    }
}
