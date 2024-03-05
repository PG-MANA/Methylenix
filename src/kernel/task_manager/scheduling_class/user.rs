//!
//! Scheduling Class for User
//!

#[derive(Clone, Copy, PartialOrd, PartialEq, Eq, Ord)]
pub struct UserSchedulingClass {}

impl UserSchedulingClass {
    pub const fn new() -> Self {
        Self {}
    }

    pub fn get_normal_priority() -> u8 {
        Self::get_custom_priority(20)
    }

    pub fn get_custom_priority(level: u8) -> u8 {
        assert!(level < 40);
        100 + level
    }

    pub(crate) fn calculate_time_slice(
        &self,
        priority_level: u8,
        number_of_threads: usize,
        interval_ms: u64,
    ) -> u64 {
        assert!((100..=140).contains(&priority_level));
        (200 * (140 - priority_level) as u64 / (number_of_threads as u64 * interval_ms)).max(10)
    }
}
