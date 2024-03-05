//!
//! Scheduling Class for Kernel
//!

#[derive(Clone, Copy, PartialOrd, PartialEq, Eq, Ord)]
pub struct KernelSchedulingClass {}

impl KernelSchedulingClass {
    pub const fn new() -> Self {
        Self {}
    }

    pub fn get_normal_priority() -> u8 {
        Self::get_custom_priority(20)
    }
    pub fn get_idle_thread_priority() -> u8 {
        0xff
    }

    pub fn get_custom_priority(level: u8) -> u8 {
        assert!(level < 40);
        80 + level
    }

    pub(crate) fn calculate_time_slice(
        &self,
        priority_level: u8,
        number_of_threads: usize,
        interval_ms: u64,
    ) -> u64 {
        if priority_level == Self::get_idle_thread_priority() {
            10
        } else {
            assert!((80..=120).contains(&priority_level));
            (200 * (120 - priority_level) as u64 / (number_of_threads as u64 * interval_ms)).max(10)
        }
    }
}
