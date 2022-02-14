//!
//! Generic Interrupt Controller version 3 and 4
//!

use crate::arch::target_arch::device::cpu;

use crate::kernel::drivers::acpi::table::madt::MadtManager;
use crate::kernel::drivers::acpi::AcpiManager;
use crate::kernel::drivers::dtb::{DtbManager, DtbNodeInfo};
use crate::kernel::memory_manager::data_type::{
    Address, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress, VAddress,
};
use crate::{free_pages, io_remap};

const GIC_V3_DISTRIBUTOR_MEMORY_MAP_SIZE: MSize = MSize::new(0x10000);

const GIC_V3_REDISTRIBUTOR_MEMORY_MAP_SIZE: MSize = MSize::new(0x20000);

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum GicV3Group {
    NonSecureEl1,
}

enum GicInformationSoruce {
    Madt(MadtManager),
    Dtb(DtbNodeInfo),
}

pub struct GicManager {
    info_source: GicInformationSoruce,
    interrupt_distributor_base_address: VAddress,
}

pub struct GicRedistributorManager {
    base_address: VAddress,
}

impl GicManager {
    const GICD_CTLR: usize = 0x00;
    const GCID_CTLR_RWP: u32 = 1 << 31;
    const GCID_CTLR_DS: u32 = 1 << 6;
    const GCID_CTLR_ENABLE_GRP1: u32 = 1;

    /* Device Tree Definitions */
    pub const DTB_GIC_SPI: u32 = 0x00;
    pub const DTB_GIC_PPI: u32 = 0x01;
    pub const DTB_GIC_SPI_INTERRUPT_ID_OFFSET: u32 = 32;

    pub fn new_with_acpi(acpi_manager: &AcpiManager) -> Option<Self> {
        if let Some(madt_manager) = acpi_manager
            .get_table_manager()
            .get_table_manager::<MadtManager>()
        {
            Some(Self {
                info_source: GicInformationSoruce::Madt(madt_manager),
                interrupt_distributor_base_address: VAddress::new(0),
            })
        } else {
            pr_err!("GICv3 or later information is not found.");
            None
        }
    }

    pub fn new_with_dtb(dtb_manager: &DtbManager) -> Option<Self> {
        let mut previous = None;
        while let Some(node_info) =
            dtb_manager.search_node(b"interrupt-controller", previous.as_ref())
        {
            if dtb_manager.is_device_compatible(&node_info, b"arm,gic-v3")
                && dtb_manager.is_node_operational(&node_info)
            {
                return Some(Self {
                    info_source: GicInformationSoruce::Dtb(node_info),
                    interrupt_distributor_base_address: VAddress::new(0),
                });
            }
            previous = Some(node_info);
        }
        pr_err!("GICv3 information is not found.");
        None
    }

    pub fn init_generic_interrupt_distributor(&mut self) -> bool {
        let base_address: VAddress;
        match &self.info_source {
            GicInformationSoruce::Madt(madt_manager) => {
                let Some(gic_distributor_info) =
                    madt_manager.find_generic_interrupt_distributor() else {
                        pr_err!("GIC Distributor information is not found.");
                        return false;
                };
                if gic_distributor_info.version < 3 {
                    pr_err!("Unsupported GIC version: {}", gic_distributor_info.version);
                    return false;
                }
                base_address = match io_remap!(
                    PAddress::new(gic_distributor_info.base_address),
                    GIC_V3_DISTRIBUTOR_MEMORY_MAP_SIZE,
                    MemoryPermissionFlags::data(),
                    MemoryOptionFlags::DEVICE_MEMORY
                ) {
                    Ok(v) => v,
                    Err(e) => {
                        pr_err!("Failed to map Generic Interrupt Distributor area: {:?}", e);
                        return false;
                    }
                };
            }
            GicInformationSoruce::Dtb(_dtb) => unimplemented!(),
        }

        self.interrupt_distributor_base_address = base_address;
        self.wait_rwp();
        self.write_register(Self::GICD_CTLR, Self::GCID_CTLR_ENABLE_GRP1);
        return true;
    }

    pub fn init_redistributor(&self) -> Option<GicRedistributorManager> {
        unsafe { cpu::set_icc_sre(cpu::get_icc_sre() | GicRedistributorManager::ICC_SRE_SRE) };
        if (unsafe { cpu::get_icc_sre() } & GicRedistributorManager::ICC_SRE_SRE) == 0 {
            pr_err!("GICv3 or later with System Registers is disabled.");
            return None;
        }
        let redistributor_address = match &self.info_source {
            GicInformationSoruce::Madt(madt) => {
                if let Some(info) = madt.find_generic_interrupt_controller_cpu_interface(
                    cpu::mpidr_to_affinity(unsafe { cpu::get_mpidr() }),
                ) {
                    info.gicr_base_address
                } else {
                    pr_err!("GIC CPU Interface Structure is not found.");
                    return None;
                }
            }
            GicInformationSoruce::Dtb(_dtb) => unimplemented!(),
        };
        let base_address = match io_remap!(
            PAddress::new(redistributor_address as usize),
            GIC_V3_REDISTRIBUTOR_MEMORY_MAP_SIZE,
            MemoryPermissionFlags::data(),
            MemoryOptionFlags::DEVICE_MEMORY
        ) {
            Ok(v) => v,
            Err(e) => {
                pr_err!(
                    "Failed to map Generic Interrupt Redistributor area: {:?}",
                    e
                );
                return None;
            }
        };
        let mut redistributor_manager = GicRedistributorManager::new(base_address);
        if !redistributor_manager.init(self.read_register(Self::GICD_CTLR)) {
            pr_err!("Failed to init GIC Redistributor.");
            let _ = free_pages!(base_address);
            return None;
        }

        return Some(redistributor_manager);
    }

    fn wait_rwp(&self) {
        while (self.read_register(Self::GICD_CTLR) & Self::GCID_CTLR_RWP) != 0 {
            core::hint::spin_loop();
        }
    }

    fn read_register(&self, register: usize) -> u32 {
        unsafe {
            core::ptr::read_volatile(
                (self.interrupt_distributor_base_address.to_usize() + register) as *const u32,
            )
        }
    }

    fn write_register(&self, register: usize, data: u32) {
        unsafe {
            core::ptr::write_volatile(
                (self.interrupt_distributor_base_address.to_usize() + register) as *mut u32,
                data,
            )
        }
    }
}

impl GicRedistributorManager {
    const ICC_SRE_SRE: u64 = 1;
    const ICC_IGRPEN1_EN: u64 = 1;

    const GICR_CTLR: usize = 0x00;
    #[allow(dead_code)]
    const GICR_CTLR_UWP: u32 = 1 << 31;
    const GICR_CTLR_RWP: u32 = 1 << 3;
    const GICR_CTLR_ENABLE_LPIS: u32 = 1 << 0;

    const GICR_WAKER: usize = 0x0014;
    const GICR_WAKER_CHILDREN_ASLEEP: u32 = 1 << 2;
    const GCIR_WAKER_PROCESSOR_SLEEP: u32 = 1 << 1;

    const GICR_IGROUPR0: usize = 0x10000 + 0x0080;
    const GICR_IPRIORITYR_BASE: usize = 0x10000 + 0x0400;
    const GICR_IGRPMODR0: usize = 0x10000 + 0x0D00;
    const GICR_ISENABLER0: usize = 0x10000 + 0x0100;
    const GICR_ICENABLER0: usize = 0x10000 + 0x0180;
    const GICR_ICFGR0: usize = 0x10000 + 0x0C00;
    const GICR_ICFGR1: usize = 0x10000 + 0x0C04;

    fn new(base_address: VAddress) -> Self {
        Self { base_address }
    }

    fn init(&mut self, gicd_ctlr: u32) -> bool {
        self.wait_rwp();
        self.write_register(Self::GICR_CTLR, Self::GICR_CTLR_ENABLE_LPIS);
        if (gicd_ctlr & GicManager::GCID_CTLR_DS) == 0 {
            self.write_register(
                Self::GICR_WAKER,
                self.read_register(Self::GICR_WAKER) & !Self::GCIR_WAKER_PROCESSOR_SLEEP,
            );
            while (self.read_register(Self::GICR_WAKER) & Self::GICR_WAKER_CHILDREN_ASLEEP) != 0 {
                core::hint::spin_loop();
            }
        }
        self.set_priority_mask(0xFF); // Allow all interrupt
        unsafe { cpu::set_icc_igrpen1(Self::ICC_IGRPEN1_EN) };
        return true;
    }

    pub const fn is_available(&self) -> bool {
        !self.base_address.is_zero()
    }

    /// Set Priority Mask
    ///
    /// If the priority of interrupt request  is higher(nearer 0), this processing element will generate interrupt.
    pub fn set_priority_mask(&self, mask: u8) {
        unsafe { cpu::set_icc_pmr(mask as u64) }
    }

    pub fn set_priority(&self, index: u32, priority: u8) {
        let register_index = ((index >> 2) as usize) * core::mem::size_of::<u32>();
        let register_offset = (index & 0b11) << 3;
        self.write_register(
            Self::GICR_IPRIORITYR_BASE + register_index,
            (self.read_register(Self::GICR_IPRIORITYR_BASE + register_index)
                & !(0xFF << register_offset))
                | ((priority as u32) << register_offset),
        );
    }

    pub fn set_group(&self, index: u32, group: GicV3Group) {
        if index > 31 {
            pr_err!("Invalid index: {:#X}", index);
            return;
        }

        let data = match group {
            GicV3Group::NonSecureEl1 => 1,
        };
        self.write_register(
            Self::GICR_IGROUPR0,
            (self.read_register(Self::GICR_IGROUPR0) & !(0x01 << index)) | ((data) << index),
        );

        let data = match group {
            GicV3Group::NonSecureEl1 => 0,
        };
        self.write_register(
            Self::GICR_IGRPMODR0,
            (self.read_register(Self::GICR_IGRPMODR0) & !(0x01 << index)) | ((data) << index),
        );
    }

    pub fn set_enable(&self, index: u32, to_enable: bool) {
        if index > 31 {
            pr_err!("Invalid index: {:#X}", index);
            return;
        }

        let register = if to_enable {
            Self::GICR_ISENABLER0
        } else {
            Self::GICR_ICENABLER0
        };

        self.write_register(register, 1 << index);
    }

    pub fn set_sgi_trigger_mode(&self, index: u32, is_level_trigger: bool) {
        if index > 31 {
            pr_err!("Invalid index: {:#X}", index);
            return;
        }
        self.write_register(
            Self::GICR_ICFGR0,
            (self.read_register(Self::GICR_ICFGR0) & !(0x01 << index))
                | (((!is_level_trigger) as u32) << index),
        );
    }

    pub fn set_ppi_trigger_mode(&self, index: u32, is_level_trigger: bool) {
        if index > 31 {
            pr_err!("Invalid index: {:#X}", index);
            return;
        }
        self.write_register(
            Self::GICR_ICFGR1,
            (self.read_register(Self::GICR_ICFGR1) & !(0x01 << index))
                | (((!is_level_trigger) as u32) << index),
        );
    }

    /*pub fn get_highest_priority_pending_interrupt(&self) -> (u32, GicV3Group) {
        (
            unsafe { cpu::get_icc_hppir1() as u32 },
            GicV3Group::NonSecureEl1,
        )
    }*/

    pub fn get_acknowledge(&self) -> (u32, GicV3Group) {
        (
            unsafe { cpu::get_icc_iar1() as u32 },
            GicV3Group::NonSecureEl1,
        )
    }

    pub fn send_eoi(&self, index: u32, group: GicV3Group) {
        match group {
            GicV3Group::NonSecureEl1 => {
                unsafe { cpu::set_icc_eoir1(index as u64) };
            }
        }
    }

    #[allow(dead_code)]
    fn wait_uwp(&self) {
        while (self.read_register(Self::GICR_CTLR) & Self::GICR_CTLR_UWP) != 0 {
            core::hint::spin_loop();
        }
    }

    fn wait_rwp(&self) {
        while (self.read_register(Self::GICR_CTLR) & Self::GICR_CTLR_RWP) != 0 {
            core::hint::spin_loop();
        }
    }

    fn read_register(&self, register: usize) -> u32 {
        unsafe { core::ptr::read_volatile((self.base_address.to_usize() + register) as *const u32) }
    }

    fn write_register(&self, register: usize, data: u32) {
        unsafe {
            core::ptr::write_volatile((self.base_address.to_usize() + register) as *mut u32, data)
        }
    }
}
