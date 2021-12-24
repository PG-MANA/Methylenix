///
/// Scheduling Class
///
pub mod kernel;
pub mod user;

use self::kernel::KernelSchedulingClass;
use self::user::UserSchedulingClass;

#[derive(Clone, Copy, PartialOrd, PartialEq, Eq, Ord)]
pub enum SchedulingClass {
    KernelThread(KernelSchedulingClass),
    UserThread(UserSchedulingClass),
}

impl SchedulingClass {
    pub(crate) fn calculate_time_slice(
        &self,
        priority_level: u8,
        number_of_threads: usize,
        interval_ms: u64,
    ) -> u64 {
        match self {
            SchedulingClass::KernelThread(s) => {
                s.calculate_time_slice(priority_level, number_of_threads, interval_ms)
            }
            SchedulingClass::UserThread(s) => {
                s.calculate_time_slice(priority_level, number_of_threads, interval_ms)
            }
        }
    }
}
