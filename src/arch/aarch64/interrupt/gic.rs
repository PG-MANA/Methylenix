//!
//! Generic Interrupt Controller
//!

use super::InterruptGroup;

use crate::kernel::drivers::{
    acpi::{AcpiManager, table::madt::MadtManager},
    dtb::{DtbManager, DtbNodeInfo},
};
use crate::kernel::memory_manager::data_type::{MSize, PAddress};

mod gicv2;
mod gicv3;

pub enum GicDistributor {
    GicV2(gicv2::GicV2Distributor),
    GicV3(gicv3::GicV3Distributor),
}

pub enum GicRedistributor {
    GicV2(gicv2::GicV2Redistributor),
    GicV3(gicv3::GicV3Redistributor),
}

/* Devicetree Definitions */
const DTB_GIC_SPI: u32 = 0x00;
const DTB_GIC_PPI: u32 = 0x01;
const DTB_GIC_PPI_INTERRUPT_ID_OFFSET: u32 = 16;
const DTB_GIC_SPI_INTERRUPT_ID_OFFSET: u32 = 32;

impl GicDistributor {
    pub const INTERRUPT_ID_INVALID: u32 = 1023;

    pub fn new_with_acpi(acpi_manager: &AcpiManager) -> Result<Self, ()> {
        let Some(madt) = acpi_manager
            .get_table_manager()
            .get_table_manager::<MadtManager>()
        else {
            pr_err!("MADT is not found.");
            return Err(());
        };
        let Some(gic_distributor_info) = madt.find_generic_interrupt_distributor() else {
            pr_err!("GIC Distributor information is not found.");
            return Err(());
        };
        pr_info!("Detect GICv{}", gic_distributor_info.version);
        match gic_distributor_info.version {
            1 | 2 => Ok(Self::GicV2(gicv2::GicV2Distributor::new(PAddress::new(
                gic_distributor_info.base_address,
            ))?)),
            3 | 4 => {
                let redistributor_range =
                    madt.find_generic_interrupt_redistributor_struct().map(|i| {
                        (
                            PAddress::new(i.discovery_range_base_address as _),
                            MSize::new(i.discovery_range_length as _),
                        )
                    });
                Ok(Self::GicV3(gicv3::GicV3Distributor::new(
                    PAddress::new(gic_distributor_info.base_address),
                    gic_distributor_info.version,
                    redistributor_range,
                )?))
            }
            v => {
                pr_err!("Unsupported GIC version: {v}");
                Err(())
            }
        }
    }

    pub fn new_redistributor_with_acpi(
        &self,
        acpi_manager: &AcpiManager,
    ) -> Option<GicRedistributor> {
        let Some(madt) = acpi_manager
            .get_table_manager()
            .get_table_manager::<MadtManager>()
        else {
            pr_err!("MADT is not found.");
            return None;
        };
        Some(match self {
            GicDistributor::GicV2(d) => GicRedistributor::GicV2(d.init_redistributor(
                |affinity| -> Option<(PAddress, u8)> {
                    madt.find_generic_interrupt_controller_cpu_interface(affinity)
                        .map(|info| {
                            (
                                PAddress::new(info.physical_address as _),
                                info.cpu_interface_number as u8,
                            )
                        })
                },
            )?),
            GicDistributor::GicV3(d) => {
                GicRedistributor::GicV3(d.init_redistributor(|affinity| -> Option<PAddress> {
                    madt.find_generic_interrupt_controller_cpu_interface(affinity)
                        .map(|info| PAddress::new(info.gicr_base_address as _))
                })?)
            }
        })
    }

    pub fn new_with_dtb(dtb_manager: &DtbManager) -> Result<Self, ()> {
        let mut current_node = None;
        while let Some(node) =
            dtb_manager.search_node(b"interrupt-controller", current_node.as_ref())
        {
            if !dtb_manager.is_node_operational(&node) {
                current_node = Some(node);
                continue;
            }
            let version = if dtb_manager.is_device_compatible(&node, b"arm,gic-v4") {
                4
            } else if dtb_manager.is_device_compatible(&node, b"arm,gic-v3") {
                3
            } else if dtb_manager.is_device_compatible(&node, b"arm,gic-v2")
                || dtb_manager.is_device_compatible(&node, b"arm,gic-400")
            {
                2
            } else {
                pr_warn!("Unknown interrupt controller.");
                current_node = Some(node);
                continue;
            };
            pr_info!("Detect GIC v{}", version);
            match version {
                3 | 4 => {
                    let Some((base, _)) = dtb_manager.read_reg_property(&node, 0) else {
                        pr_err!("Failed to read reg property");
                        return Err(());
                    };
                    let redistributor_range = dtb_manager
                        .read_reg_property(&node, 1)
                        .map(|(base, size)| (PAddress::new(base), MSize::new(size)));
                    return Ok(Self::GicV3(gicv3::GicV3Distributor::new(
                        PAddress::new(base),
                        version,
                        redistributor_range,
                    )?));
                }
                2 => {
                    let Some((base, _)) = dtb_manager.read_reg_property(&node, 0) else {
                        pr_err!("Failed to read reg property");
                        return Err(());
                    };
                    return Ok(Self::GicV2(gicv2::GicV2Distributor::new(PAddress::new(
                        base,
                    ))?));
                }
                _ => unreachable!(),
            }
        }
        pr_err!("GIC is not found");
        Err(())
    }

    pub fn new_redistributor_with_dtb(&self, dtb_manager: &DtbManager) -> Option<GicRedistributor> {
        Some(match self {
            GicDistributor::GicV2(d) => GicRedistributor::GicV2(d.init_redistributor(
                |affinity| -> Option<(PAddress, u8)> {
                    /* Not a good algorithm */
                    let mut current_node = None;
                    let mut result = None;
                    while let Some(node) =
                        dtb_manager.search_node(b"interrupt-controller", current_node.as_ref())
                    {
                        if dtb_manager.is_node_operational(&node)
                            && (dtb_manager.is_device_compatible(&node, b"arm,gic-v2")
                                || dtb_manager.is_device_compatible(&node, b"arm,gic-400"))
                        {
                            let cpu_interface = (affinity & 0xF) as u8;
                            if let Some((base, _)) =
                                dtb_manager.read_reg_property(&node, 1 + cpu_interface as usize)
                            {
                                result = Some((PAddress::new(base), cpu_interface));
                                break;
                            }
                        }
                        current_node = Some(node);
                    }
                    result
                },
            )?),
            GicDistributor::GicV3(d) => GicRedistributor::GicV3(d.init_redistributor(|_| None)?),
        })
    }

    pub fn init(&mut self) -> bool {
        match self {
            GicDistributor::GicV2(d) => d.init(),
            GicDistributor::GicV3(d) => d.init(),
        }
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
    pub fn set_priority_mask(&self, mask: u8, group: InterruptGroup) {
        match self {
            GicRedistributor::GicV2(r) => r.set_priority_mask(mask, group),
            GicRedistributor::GicV3(r) => r.set_priority_mask(mask, group),
        }
    }

    pub fn set_binary_point(&self, point: u8, group: InterruptGroup) {
        match self {
            GicRedistributor::GicV2(r) => r.set_binary_point(point, group),
            GicRedistributor::GicV3(r) => r.set_binary_point(point, group),
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

pub fn read_interrupt_info_from_dtb(
    dtb_manager: &DtbManager,
    info: &DtbNodeInfo,
    index: usize,
) -> Option<(
    u32,  /* interrupt_id */
    bool, /* is_level_trigger */
)> {
    let base = index * 3;
    dtb_manager
        .get_property(info, &DtbManager::PROP_INTERRUPTS)
        .and_then(|i| {
            if let Some(interrupt_type) = dtb_manager.read_property_as_u32(&i, base)
                && let Some(interrupt_id) = dtb_manager.read_property_as_u32(&i, base + 1)
                && let Some(interrupt_attribute) = dtb_manager.read_property_as_u32(&i, base + 2)
            {
                Some((
                    match interrupt_type {
                        DTB_GIC_SPI => DTB_GIC_SPI_INTERRUPT_ID_OFFSET + interrupt_id,
                        DTB_GIC_PPI => DTB_GIC_PPI_INTERRUPT_ID_OFFSET + interrupt_id,
                        t => {
                            pr_warn!("Unknown interrupt type: {t:#X}");
                            interrupt_id
                        }
                    },
                    interrupt_attribute & 0b1111 == 4,
                ))
            } else {
                None
            }
        })
}
