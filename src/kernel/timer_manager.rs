//!
//! Timer Manager
//!
//! This manager is the frontend of timer system.
//! Arch-specific timer call this manager to process timer queue.
//! When task-switch, this will return to arch-specific timer function once
//! for processes like sending end of interrupt.
//! After that, the timer should recall this manager.
//! The member of this manager may be changed.

use crate::arch::target_arch::device::cpu::is_interrupt_enabled;

use crate::kernel::sync::spin_lock::SpinLockFlag;

pub struct TimerManager {
    tick: usize,
    lock: SpinLockFlag,
}

impl TimerManager {
    pub const TIMER_INTERVAL_MS: usize = 10;

    pub fn new() -> Self {
        Self {
            tick: 0,
            lock: SpinLockFlag::new(),
        }
    }

    pub fn timer_handler(&mut self) {
        if is_interrupt_enabled() {
            pr_err!("Interrupt is enabled.");
            return;
        }
        let _lock = if let Ok(t) = self.lock.try_lock() {
            t
        } else {
            pr_err!("Cannot lock Timer Manager.");
            return;
        };
        self.tick = self.tick.overflowing_add(1).0;
    }

    fn get_end_tick_ms(current_tick: usize, ms: usize) -> (usize, bool) {
        current_tick.overflowing_add(ms / Self::TIMER_INTERVAL_MS)
    }

    pub fn get_current_tick_without_lock(&self) -> usize {
        self.tick
    }

    pub fn get_difference_ms(&self, tick: usize) -> usize {
        let current_tick = self.get_current_tick_without_lock();
        let difference = if current_tick < tick {
            usize::MAX - tick + current_tick
        } else {
            current_tick - tick
        };
        difference * Self::TIMER_INTERVAL_MS
    }

    pub fn busy_wait_ms(&self, ms: usize) -> bool {
        let start_tick = self.get_current_tick_without_lock(); /* get_quickly */
        if !is_interrupt_enabled() {
            pr_err!("Interrupt is disabled.");
            return false;
        }
        let (end_tick, overflowed) = Self::get_end_tick_ms(start_tick, ms);
        if overflowed {
            while self.get_current_tick_without_lock() >= start_tick {
                core::hint::spin_loop();
            }
        }

        while self.get_current_tick_without_lock() <= end_tick {
            core::hint::spin_loop();
        }
        return true;
    }
}

pub trait Timer {
    fn get_count(&self) -> usize;
    fn get_frequency_hz(&self) -> usize;
    fn is_count_up_timer(&self) -> bool;
    fn get_difference(&self, earlier: usize, later: usize) -> usize;
    fn get_ending_count_value(&self, start: usize, difference: usize) -> usize;
    fn get_max_counter_value(&self) -> usize;

    #[inline(always)]
    fn busy_wait_ms(&self, ms: usize) {
        let start = self.get_count();
        let difference = self.get_frequency_hz() * ms / 1000;
        if difference > self.get_max_counter_value() {
            panic!("Cannot count more than max_counter_value");
        }
        let end = self.get_ending_count_value(start, difference);
        self.wait_until(start, end);
    }

    #[inline(always)]
    fn busy_wait_us(&self, us: usize) {
        let start = self.get_count();
        let difference = self.get_frequency_hz() * us / 1000000;
        if difference > self.get_max_counter_value() {
            panic!("Cannot count more than max_counter_value");
        } else if difference == 0 {
            panic!("Cannot count less than the resolution");
        }
        let end = self.get_ending_count_value(start, difference);
        self.wait_until(start, end);
    }

    #[inline(always)]
    fn wait_until(&self, start_counter_value: usize, end_counter_value: usize) {
        use core::hint::spin_loop;
        if self.is_count_up_timer() {
            if start_counter_value > end_counter_value {
                while self.get_count() >= start_counter_value {
                    spin_loop();
                }
            }
            while self.get_count() < end_counter_value {
                spin_loop();
            }
        } else {
            if start_counter_value < end_counter_value {
                while self.get_count() <= start_counter_value {
                    spin_loop();
                }
            }
            while self.get_count() > end_counter_value {
                spin_loop();
            }
        }
    }
}
