//!
//! ACPI Event Handler
//!

use super::table::fadt::FadtManager;

use crate::arch::target_arch::device::cpu::{in_word, out_word};

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::sync::spin_lock::SpinLockFlag;

#[derive(Copy, Clone, Debug)]
#[repr(u16)]
pub enum AcpiFixedEvent {
    Global = 1 << 5,
    PowerButton = 1 << 8,
    SleepButton = 1 << 9,
}

impl AcpiFixedEvent {
    pub fn from_u16(e: u16) -> Option<Self> {
        if (e & AcpiFixedEvent::PowerButton as u16) != 0 {
            Some(Self::PowerButton)
        } else if (e & AcpiFixedEvent::SleepButton as u16) != 0 {
            Some(Self::SleepButton)
        } else {
            None
        }
    }
}

pub struct AcpiEventManager {
    write_lock: SpinLockFlag,
    pm1a_event_block: usize,
    pm1b_event_block: usize,
    pm1_event_block_len: u8,
    pm1a_enabled_event: u16,
    pm1b_enabled_event: u16,
}

impl AcpiEventManager {
    pub fn new(fadt_manager: &FadtManager) -> Self {
        Self {
            write_lock: SpinLockFlag::new(),
            pm1a_event_block: fadt_manager.get_pm1a_event_block(),
            pm1b_event_block: fadt_manager.get_pm1b_event_block(),
            pm1_event_block_len: fadt_manager.get_pm1_event_block_len(),
            pm1a_enabled_event: 0,
            pm1b_enabled_event: 0,
        }
    }

    fn read_pm1_a_status(&self) -> u16 {
        unsafe { in_word(self.pm1a_event_block as _) }
    }

    fn read_pm1_b_status(&self) -> u16 {
        unsafe { in_word(self.pm1b_event_block as _) }
    }

    fn read_pm1_a_enable(&self) -> u16 {
        unsafe { in_word((self.pm1a_event_block + (self.pm1_event_block_len / 2) as usize) as _) }
    }

    fn read_pm1_b_enable(&self) -> u16 {
        unsafe { in_word((self.pm1b_event_block + (self.pm1_event_block_len / 2) as usize) as _) }
    }

    fn write_pm1_a_status(&self, d: u16) {
        unsafe { out_word(self.pm1a_event_block as _, d) }
    }

    fn write_pm1_b_status(&self, d: u16) {
        unsafe { out_word(self.pm1b_event_block as _, d) }
    }

    fn write_pm1_a_enable(&self, d: u16) {
        unsafe {
            out_word(
                (self.pm1a_event_block + (self.pm1_event_block_len / 2) as usize) as _,
                d,
            )
        }
    }

    fn write_pm1_b_enable(&self, d: u16) {
        unsafe {
            out_word(
                (self.pm1b_event_block + (self.pm1_event_block_len / 2) as usize) as _,
                d,
            )
        }
    }

    pub fn enable_fixed_event(&mut self, event: AcpiFixedEvent) -> bool {
        if (self.read_pm1_a_enable() & event as u16) != 0
            && (self.read_pm1_b_enable() & event as u16) != 0
        {
            return true;
        }
        let _lock = if let Ok(l) = self.write_lock.try_lock() {
            l
        } else {
            return false;
        };
        self.pm1a_enabled_event |= event as u16;
        self.pm1b_enabled_event |= event as u16;
        self.write_pm1_a_enable(self.pm1a_enabled_event);
        if self.pm1b_event_block != 0 {
            self.write_pm1_b_enable(self.pm1b_enabled_event);
        }
        return true;
    }

    pub fn find_occurred_fixed_event(&self) -> Option<AcpiFixedEvent> {
        let pm1a_status = self.read_pm1_a_status() & self.pm1a_enabled_event;
        let result = AcpiFixedEvent::from_u16(pm1a_status);
        if result.is_none() && self.pm1b_event_block != 0 {
            AcpiFixedEvent::from_u16(self.read_pm1_b_status() & self.pm1b_enabled_event)
        } else {
            result
        }
    }

    pub fn reset_fixed_event_status(&self, event: AcpiFixedEvent) -> bool {
        let _lock = if let Ok(l) = self.write_lock.try_lock() {
            l
        } else {
            return false;
        };
        self.write_pm1_a_status(event as u16);
        if self.pm1b_event_block != 0 {
            self.write_pm1_b_status(event as u16);
        }
        return true;
    }

    pub fn acpi_fixed_event_worker(event: usize) {
        if let Some(event) = AcpiFixedEvent::from_u16(event as u16) {
            match event {
                AcpiFixedEvent::PowerButton => {
                    pr_info!("Power Button was pushed.");
                    if let Ok(mut m) = get_kernel_manager_cluster().acpi_manager.try_lock() {
                        m.shutdown_test()
                    } else {
                        pr_err!("Cannot lock ACPI Manager.");
                    }
                }
                AcpiFixedEvent::SleepButton => {
                    pr_info!("Sleep Button");
                }
                AcpiFixedEvent::Global => {
                    pr_info!("Global Event!");
                }
            }
        } else {
            pr_err!("Unknown event...");
        }
    }
}
