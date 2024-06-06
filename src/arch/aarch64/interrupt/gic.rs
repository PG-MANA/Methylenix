//!
//! Generic Interrupt Controller
//!

use super::gicv2;
use super::gicv3;
use super::InterruptGroup;

use crate::kernel::drivers::acpi::{table::madt::MadtManager, AcpiManager};
use crate::kernel::memory_manager::data_type::PAddress;

pub enum GicDistributor {
    GicV2(gicv2::GicV2Distributor),
    GicV3(gicv3::GicV3Distributor),
}

pub enum GicRedistributor {
    GicV2(gicv2::GicV2Redistributor),
    GicV3(gicv3::GicV3Redistributor),
}

impl GicDistributor {
    pub const INTERRUPT_ID_INVALID: u32 = 1023;
    /* Device Tree Definitions */
    pub const DTB_GIC_SPI: u32 = 0x00;
    pub const DTB_GIC_PPI: u32 = 0x01;
    pub const DTB_GIC_SPI_INTERRUPT_ID_OFFSET: u32 = 32;

    pub fn new_with_acpi(acpi_manager: &AcpiManager) -> Result<Self, ()> {
        let Some(madt) = acpi_manager
            .get_table_manager()
            .get_table_manager::<MadtManager>()
        else {
            pr_err!("GIC is not found.");
            return Err(());
        };
        let Some(gic_distributor_info) = madt.find_generic_interrupt_distributor() else {
            pr_err!("GIC Distributor information is not found.");
            return Err(());
        };
        match gic_distributor_info.version {
            1 | 2 => Ok(Self::GicV2(gicv2::GicV2Distributor::new_from_acpi(
                &madt,
                gic_distributor_info,
            )?)),
            3 | 4 => Ok(Self::GicV3(gicv3::GicV3Distributor::new_from_acpi(
                &madt,
                gic_distributor_info,
            )?)),
            v => {
                pr_err!("Unsupported GIC version: {v}");
                Err(())
            }
        }
    }

    pub fn init_generic_interrupt_distributor(&mut self) -> bool {
        match self {
            GicDistributor::GicV2(d) => d.init(),
            GicDistributor::GicV3(d) => d.init(),
        }
    }

    pub fn init_redistributor(&self, acpi: Option<&AcpiManager>) -> Option<GicRedistributor> {
        let madt = acpi.and_then(|a| a.get_table_manager().get_table_manager::<MadtManager>());
        Some(match self {
            GicDistributor::GicV2(d) => {
                GicRedistributor::GicV2(d.init_redistributor(madt.as_ref())?)
            }
            GicDistributor::GicV3(d) => {
                GicRedistributor::GicV3(d.init_redistributor(madt.as_ref())?)
            }
        })
    }

    pub fn set_priority(&self, index: u32, priority: u8) {
        match self {
            GicDistributor::GicV2(d) => d.set_priority(index, priority),
            GicDistributor::GicV3(d) => d.set_priority(index, priority),
        }
    }

    pub fn set_group(&self, index: u32, group: InterruptGroup) {
        match self {
            GicDistributor::GicV2(d) => d.set_group(index, group),
            GicDistributor::GicV3(d) => d.set_group(index, group),
        }
    }

    pub fn set_enable(&self, index: u32, enable: bool) {
        match self {
            GicDistributor::GicV2(d) => d.set_enable(index, enable),
            GicDistributor::GicV3(d) => d.set_enable(index, enable),
        }
    }

    pub fn set_trigger_mode(&self, index: u32, is_level_trigger: bool) {
        match self {
            GicDistributor::GicV2(d) => d.set_trigger_mode(index, is_level_trigger),
            GicDistributor::GicV3(d) => d.set_trigger_mode(index, is_level_trigger),
        }
    }

    pub fn set_routing_to_this(&self, interrupt_id: u32, is_routing_mode: bool) {
        match self {
            GicDistributor::GicV2(d) => d.set_routing_to_this(interrupt_id),
            GicDistributor::GicV3(d) => d.set_routing_to_this(interrupt_id, is_routing_mode),
        }
    }

    /// For MSI
    pub fn get_pending_register_address_and_data(&self, interrupt_id: u32) -> (PAddress, u8) {
        match self {
            GicDistributor::GicV2(d) => d.get_pending_register_address_and_data(interrupt_id),
            GicDistributor::GicV3(d) => d.get_pending_register_address_and_data(interrupt_id),
        }
    }

    pub fn send_sgi(&self, cpu_id: usize, interrupt_id: u32) {
        match self {
            GicDistributor::GicV2(d) => d.send_sgi(cpu_id, interrupt_id),
            GicDistributor::GicV3(d) => d.send_sgi(cpu_id, interrupt_id),
        }
    }
}

impl GicRedistributor {
    /// Set Priority Mask
    ///
    /// If the priority of interrupt request  is higher(nearer 0), this processing element will generate interrupt.
    pub fn set_priority_mask(&self, mask: u8) {
        match self {
            GicRedistributor::GicV2(r) => r.set_priority_mask(mask),
            GicRedistributor::GicV3(r) => r.set_priority_mask(mask),
        }
    }

    pub fn set_binary_point(&self, point: u8) {
        match self {
            GicRedistributor::GicV2(r) => r.set_binary_point(point),
            GicRedistributor::GicV3(r) => r.set_binary_point(point),
        }
    }

    pub fn set_priority(&self, index: u32, priority: u8) {
        match self {
            GicRedistributor::GicV2(r) => r.set_priority(index, priority),
            GicRedistributor::GicV3(r) => r.set_priority(index, priority),
        }
    }

    pub fn set_group(&self, index: u32, group: InterruptGroup) {
        match self {
            GicRedistributor::GicV2(r) => r.set_group(index, group),
            GicRedistributor::GicV3(r) => r.set_group(index, group),
        }
    }

    pub fn set_enable(&self, index: u32, to_enable: bool) {
        match self {
            GicRedistributor::GicV2(r) => r.set_enable(index, to_enable),
            GicRedistributor::GicV3(r) => r.set_enable(index, to_enable),
        }
    }

    pub fn set_trigger_mode(&self, index: u32, is_level_trigger: bool) {
        match self {
            GicRedistributor::GicV2(r) => r.set_trigger_mode(index, is_level_trigger),
            GicRedistributor::GicV3(r) => r.set_trigger_mode(index, is_level_trigger),
        }
    }

    pub fn get_acknowledge(&self) -> (u32, InterruptGroup) {
        match self {
            GicRedistributor::GicV2(r) => r.get_acknowledge(),
            GicRedistributor::GicV3(r) => r.get_acknowledge(),
        }
    }

    pub fn send_eoi(&self, index: u32, group: InterruptGroup) {
        match self {
            GicRedistributor::GicV2(r) => r.send_eoi(index, group),
            GicRedistributor::GicV3(r) => r.send_eoi(index, group),
        }
    }
}
