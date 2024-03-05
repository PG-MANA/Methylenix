//!
//! ACPI Event Handler
//!

pub mod gpe;

use self::gpe::GpeManager;

use super::aml::notify::NotifyList;
use super::table::fadt::FadtManager;

use crate::arch::target_arch::device::acpi::{read_io_word, write_io_word};

use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::sync::spin_lock::SpinLockFlag;
use crate::kernel::task_manager::work_queue::WorkList;

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
    gpe0_manager: GpeManager,
    gpe1_manager: Option<GpeManager>,
    notify_list: NotifyList,
}

impl AcpiEventManager {
    pub fn new(fadt_manager: &FadtManager) -> Self {
        let gpe1 = fadt_manager.get_gp_event1_block();
        let gpe1_len = fadt_manager.get_gp_event1_block_len() as usize;
        let gpe1_manager = if gpe1 != 0 && gpe1_len != 0 {
            Some(GpeManager::new(
                gpe1,
                gpe1_len >> 1, /* / 2 */
                (fadt_manager.get_gp_event0_block_len() as usize >> 1) << 3,
            ))
        } else {
            None
        };
        Self {
            write_lock: SpinLockFlag::new(),
            pm1a_event_block: fadt_manager.get_pm1a_event_block(),
            pm1b_event_block: fadt_manager.get_pm1b_event_block(),
            pm1_event_block_len: fadt_manager.get_pm1_event_block_len(),
            pm1a_enabled_event: 0,
            pm1b_enabled_event: 0,
            gpe0_manager: GpeManager::new(
                fadt_manager.get_gp_event0_block(),
                fadt_manager.get_gp_event0_block_len() as usize >> 1,
                0,
            ),
            gpe1_manager,
            notify_list: NotifyList::new(),
        }
    }

    pub fn init_event_registers(&mut self) {
        self.pm1a_enabled_event = 0;
        if self.pm1a_event_block != 0 {
            self.write_pm1_a_enable(0);
        }
        self.pm1b_enabled_event = 0;
        if self.pm1b_event_block != 0 {
            self.write_pm1_b_enable(0);
        }
        self.gpe0_manager.init();
        if let Some(gpe1) = &self.gpe1_manager {
            gpe1.init();
        }
    }

    pub fn enable_gpes(&self) -> bool {
        /* Temporary, enable EC only */
        if let Some(ec) = get_kernel_manager_cluster().acpi_device_manager.ec.as_ref() {
            if let Some(ec_gpe) = ec.get_gpe_number() {
                if ec_gpe > self.gpe0_manager.get_gpe_max_number() {
                    if !self
                        .gpe1_manager
                        .as_ref()
                        .map(|m| m.enable_gpe(ec_gpe))
                        .unwrap_or(false)
                    {
                        return false;
                    }
                } else if !self.gpe0_manager.enable_gpe(ec_gpe) {
                    return false;
                }
            }
        }
        true
    }

    pub fn clear_gpe_status_bit(&self, gpe: usize) {
        if !self.gpe0_manager.clear_status_bit(gpe) {
            if let Some(gpe1) = &self.gpe1_manager {
                gpe1.clear_status_bit(gpe);
            }
        }
    }

    pub fn get_notify_list(&self) -> &NotifyList {
        &self.notify_list
    }

    fn read_pm1_a_status(&self) -> u16 {
        read_io_word(self.pm1a_event_block as _)
    }

    fn read_pm1_b_status(&self) -> u16 {
        read_io_word(self.pm1b_event_block as _)
    }

    fn read_pm1_a_enable(&self) -> u16 {
        read_io_word((self.pm1a_event_block + (self.pm1_event_block_len / 2) as usize) as _)
    }

    fn read_pm1_b_enable(&self) -> u16 {
        read_io_word((self.pm1b_event_block + (self.pm1_event_block_len / 2) as usize) as _)
    }

    fn write_pm1_a_status(&self, d: u16) {
        write_io_word(self.pm1a_event_block as _, d)
    }

    fn write_pm1_b_status(&self, d: u16) {
        write_io_word(self.pm1b_event_block as _, d)
    }

    fn write_pm1_a_enable(&self, d: u16) {
        write_io_word(
            (self.pm1a_event_block + (self.pm1_event_block_len / 2) as usize) as _,
            d,
        )
    }

    fn write_pm1_b_enable(&self, d: u16) {
        write_io_word(
            (self.pm1b_event_block + (self.pm1_event_block_len / 2) as usize) as _,
            d,
        )
    }

    pub fn enable_fixed_event(&mut self, event: AcpiFixedEvent) -> bool {
        if self.pm1a_event_block == 0 && self.pm1b_event_block == 0 {
            return true;
        }
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
        true
    }

    pub fn find_occurred_fixed_event(&self) -> Option<AcpiFixedEvent> {
        let mut result = None;
        if self.pm1a_event_block != 0 {
            let pm1a_status = self.read_pm1_a_status() & self.pm1a_enabled_event;
            result = AcpiFixedEvent::from_u16(pm1a_status);
        }
        if result.is_none() && self.pm1b_event_block != 0 {
            AcpiFixedEvent::from_u16(self.read_pm1_b_status() & self.pm1b_enabled_event)
        } else {
            result
        }
    }

    pub fn sci_handler(&self) {
        if let Some(acpi_event) = self.find_occurred_fixed_event() {
            let work = WorkList::new(AcpiEventManager::acpi_fixed_event_worker, acpi_event as _);
            if get_cpu_manager_cluster().work_queue.add_work(work).is_err() {
                pr_err!("Failed to add work for ACPI FixedEvent({:?})", acpi_event);
            }
            if !get_kernel_manager_cluster()
                .acpi_event_manager
                .reset_fixed_event_status(acpi_event)
            {
                pr_err!("Failed to reset the Fixed Event: {:?}", acpi_event);
            }
            return;
        } else if let Some(gpe_number) = self.gpe0_manager.find_general_purpose_event(None) {
            let mut next_gpe = Some(gpe_number);
            while let Some(gpe_number) = next_gpe {
                if get_kernel_manager_cluster()
                    .acpi_device_manager
                    .get_embedded_controller()
                    .and_then(|ec| ec.get_gpe_number())
                    == Some(gpe_number)
                {
                    let query = get_kernel_manager_cluster()
                        .acpi_device_manager
                        .get_embedded_controller()
                        .unwrap()
                        .read_query();
                    if get_cpu_manager_cluster()
                        .work_queue
                        .add_work(WorkList::new(
                            AcpiEventManager::acpi_query_event_worker,
                            query as _,
                        ))
                        .is_err()
                    {
                        pr_err!("Failed to add work for ACPI Query({:#X})", query);
                    }
                } else if get_cpu_manager_cluster()
                    .work_queue
                    .add_work(WorkList::new(
                        AcpiEventManager::acpi_gpe_worker,
                        gpe_number as _,
                    ))
                    .is_err()
                {
                    pr_err!("Failed to add work for ACPI GPE({:#X})", gpe_number);
                }
                self.gpe0_manager.clear_status_bit(gpe_number);
                next_gpe = self.gpe0_manager.find_general_purpose_event(next_gpe);
            }
            return;
        }
        if let Some(ec) = &get_kernel_manager_cluster().acpi_device_manager.ec {
            let query = ec.read_query();
            if query != 0 {
                if get_cpu_manager_cluster()
                    .work_queue
                    .add_work(WorkList::new(
                        AcpiEventManager::acpi_query_event_worker,
                        query as _,
                    ))
                    .is_err()
                {
                    pr_err!("Failed to add work for ACPI Query({:#X})", query);
                }
                return;
            }
        }
        pr_err!("Unknown ACPI Event");
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
        true
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

    pub fn acpi_gpe_worker(gpe_number: usize) {
        let acpi_manager = get_kernel_manager_cluster().acpi_manager.lock().unwrap();
        pr_debug!("GPE: {:#X}", gpe_number);
        let _ = acpi_manager.evaluate_edge_trigger_event(gpe_number as u8);
        let _ = acpi_manager.evaluate_level_trigger_event(gpe_number as u8);
    }

    pub fn acpi_query_event_worker(query: usize) {
        let acpi_manager = get_kernel_manager_cluster().acpi_manager.lock().unwrap();
        pr_debug!("Query: {:#X}", query);
        acpi_manager.evaluate_query(query as u8);
    }
}
