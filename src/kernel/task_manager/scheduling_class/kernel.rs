//!
//! Scheduling Class for Kernel
//!

pub struct KernelSchedulingClass {}

impl KernelSchedulingClass {
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
}
