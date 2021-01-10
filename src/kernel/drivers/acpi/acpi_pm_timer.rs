//!
//! ACPI Power Management Timer
//!
//! ACPI Power Management Timer(ACPI PM Timer) is the timer equipped in ACPI.
//! Its frequency is 3.579545 MHz.

use crate::arch::target_arch::device::cpu;

use crate::kernel::timer_manager::Timer;

pub struct AcpiPmTimer {
    port: usize,
    is_32_bit_counter: bool,
}

impl AcpiPmTimer {
    pub const fn new(port: usize, is_32_bit_counter: bool) -> Self {
        Self {
            port,
            is_32_bit_counter,
        }
    }
}

impl Timer for AcpiPmTimer {
    fn get_count(&self) -> usize {
        let mut result = unsafe { cpu::in_dword(self.port as _) };
        if self.is_32_bit_counter == false {
            result &= 0xffffff;
        }
        result as usize
    }

    fn get_frequency_hz(&self) -> usize {
        3579545
    }

    fn is_count_up_timer(&self) -> bool {
        true
    }

    fn get_difference(&self, earlier: usize, later: usize) -> usize {
        if earlier <= later {
            later - earlier
        } else if self.is_32_bit_counter {
            later + (0xffffffff - earlier)
        } else {
            later + (0xffffff - earlier)
        }
    }

    fn get_ending_count_value(&self, start: usize, difference: usize) -> usize {
        let (result, overflow) = start.overflowing_add(difference);

        if self.is_32_bit_counter {
            result
        } else if overflow == false {
            if result <= 0xffffff {
                result
            } else {
                result - 0xffffff
            }
        } else {
            result + (0xffffffff - 0xffffff)
        }
    }

    fn get_max_counter_value(&self) -> usize {
        if self.is_32_bit_counter {
            0xffffffff
        } else {
            0xffffff
        }
    }
}
