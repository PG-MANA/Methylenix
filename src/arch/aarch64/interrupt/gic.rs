//!
//! Generic Interrupt Controller version 3 and 4
//!

use crate::arch::target_arch::device::cpu;

use crate::io_remap;
use crate::kernel::drivers::acpi::table::madt::MadtManager;
use crate::kernel::drivers::acpi::AcpiManager;
use crate::kernel::drivers::dtb::{DtbManager, DtbNodeInfo};
use crate::kernel::memory_manager::data_type::{
    Address, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress, VAddress,
};

const GIC_V3_DISTRIBUTOR_MEMORY_MAP_SIZE: MSize = MSize::new(0x10000);

const GIC_V3_REDISTRIBUTOR_MEMORY_MAP_SIZE: MSize = MSize::new(0x20000);
const GIC_V4_REDISTRIBUTOR_MEMORY_MAP_SIZE: MSize = MSize::new(0x40000);

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
    interrupt_redistributor_discovery_base_address: Option<VAddress>,
    interrupt_redistributor_discovery_length: u32,
    version: u8,
}

pub struct GicRedistributorManager {
    base_address: VAddress,
}

impl GicManager {
    const GICD_CTLR: usize = 0x00;
    const GCID_CTLR_RWP: u32 = 1 << 31;
    //const GCID_CTLR_DS: u32 = 1 << 6;
    const GICD_CTLR_ARE: u32 = 1 << 5;
    const GCID_CTLR_ENABLE_GRP1NS: u32 = 1 << 1;
    const GCID_CTLR_ENABLE_GRP0: u32 = 1 << 1;
    const GICD_IGROUPR: usize = 0x0080;
    const GICD_ISENABLER: usize = 0x1000;
    const GICD_ICENABLER: usize = 0x1080;
    const GICD_IPRIORITYR: usize = 0x0400;
    const GICD_ITARGETSR: usize = 0x0800;

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
                interrupt_redistributor_discovery_base_address: None,
                interrupt_redistributor_discovery_length: 0,
                version: 0,
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
                    interrupt_redistributor_discovery_base_address: None,
                    interrupt_redistributor_discovery_length: 0,
                    version: 3,
                });
            }
            previous = Some(node_info);
        }
        pr_err!("GICv3 information is not found.");
        None
    }

    pub fn init_generic_interrupt_distributor(&mut self) -> bool {
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
                self.version = gic_distributor_info.version;
                let base_address = match io_remap!(
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
                self.interrupt_distributor_base_address = base_address;
                if let Some(discovery_info) =
                    madt_manager.find_generic_interrupt_redistributor_struct()
                {
                    let discovery_info_base_address = match io_remap!(
                        PAddress::new(discovery_info.discovery_range_base_address as usize),
                        MSize::new(discovery_info.discovery_range_length as usize),
                        MemoryPermissionFlags::data(),
                        MemoryOptionFlags::DEVICE_MEMORY
                    ) {
                        Ok(v) => v,
                        Err(e) => {
                            pr_err!(
                                "Failed to map Generic Interrupt Redistributor area: {:?}",
                                e
                            );
                            return false;
                        }
                    };
                    self.interrupt_redistributor_discovery_base_address =
                        Some(discovery_info_base_address);
                    self.interrupt_redistributor_discovery_length =
                        discovery_info.discovery_range_length;
                }
            }
            GicInformationSoruce::Dtb(_dtb) => unimplemented!(),
        }

        self.write_register(Self::GICD_CTLR, Self::GICD_CTLR_ARE);
        self.wait_rwp();
        self.write_register(
            Self::GICD_CTLR,
            Self::GICD_CTLR_ARE | Self::GCID_CTLR_ENABLE_GRP1NS | Self::GCID_CTLR_ENABLE_GRP0,
        );
        return true;
    }

    pub fn init_redistributor(&self) -> Option<GicRedistributorManager> {
        let redistributor_address = self.find_redistributor_address_of_this_pe()?;
        let mut redistributor_manager = GicRedistributorManager::new(redistributor_address);
        if !redistributor_manager.init() {
            pr_err!("Failed to init GIC Redistributor.");
            return None;
        }
        return Some(redistributor_manager);
    }

    fn find_redistributor_address_of_this_pe(&self) -> Option<VAddress> {
        if let Some(discovery_base) = self.interrupt_redistributor_discovery_base_address {
            let mut pointer = 0;
            let redistributor_memory_size = if self.version == 3 {
                GIC_V3_REDISTRIBUTOR_MEMORY_MAP_SIZE.to_usize()
            } else if self.version == 4 {
                GIC_V4_REDISTRIBUTOR_MEMORY_MAP_SIZE.to_usize()
            } else {
                unreachable!()
            };
            let self_affinity = cpu::mpidr_to_packed_affinity(unsafe { cpu::get_mpidr() });
            while pointer < (self.interrupt_redistributor_discovery_length as usize) {
                let base = discovery_base.to_usize() + pointer;
                if unsafe { *((base + GicRedistributorManager::GICR_TYPER_AFFINITY) as *const u32) }
                    == self_affinity
                {
                    return Some(VAddress::new(base));
                }
                pointer += redistributor_memory_size;
            }
            pr_err!(
                "GIC Redistributor for affinity({:#X}) is not found.",
                self_affinity
            );
            return None;
        } else {
            match &self.info_source {
                GicInformationSoruce::Madt(madt) => {
                    return if let Some(info) = madt.find_generic_interrupt_controller_cpu_interface(
                        cpu::mpidr_to_affinity(unsafe { cpu::get_mpidr() }),
                    ) {
                        match io_remap!(
                            PAddress::new(info.gicr_base_address as usize),
                            if self.version == 3 {
                                GIC_V3_REDISTRIBUTOR_MEMORY_MAP_SIZE
                            } else if self.version == 4 {
                                GIC_V4_REDISTRIBUTOR_MEMORY_MAP_SIZE
                            } else {
                                unimplemented!()
                            },
                            MemoryPermissionFlags::data(),
                            MemoryOptionFlags::DEVICE_MEMORY
                        ) {
                            Ok(v) => Some(v),
                            Err(e) => {
                                pr_err!(
                                    "Failed to map Generic Interrupt Redistributor area: {:?}",
                                    e
                                );
                                return None;
                            }
                        }
                    } else {
                        pr_err!("GIC CPU Interface Structure is not found.");
                        None
                    }
                }
                GicInformationSoruce::Dtb(_dtb) => unimplemented!(),
            }
        }
    }

    pub fn set_priority(&self, index: u32, priority: u8) {
        let register_index = ((index >> 2) as usize) * core::mem::size_of::<u32>();
        let register_offset = (index & 0b11) << 3;
        self.write_register(
            Self::GICD_IPRIORITYR + register_index,
            (self.read_register(Self::GICD_IPRIORITYR + register_index)
                & !(0xFF << register_offset))
                | ((priority as u32) << register_offset),
        );
    }

    pub fn set_target(&self, index: u32, target: u8) {
        let register_index = ((index >> 2) as usize) * core::mem::size_of::<u32>();
        let register_offset = (index & 0b11) << 3;
        self.write_register(
            Self::GICD_ITARGETSR + register_index,
            (self.read_register(Self::GICD_ITARGETSR + register_index)
                & !(0xFF << register_offset))
                | ((target as u32) << register_offset),
        );
    }

    pub fn set_group(&self, index: u32, group: GicV3Group) {
        let register_index = (index / u32::BITS) as usize;
        let register_offset = index & (u32::BITS - 1);
        let data = match group {
            GicV3Group::NonSecureEl1 => 1,
        };
        self.write_register(
            Self::GICD_IGROUPR + register_index,
            (self.read_register(Self::GICD_IGROUPR + register_index) & !(1 << register_offset))
                | (data << register_offset),
        );
    }

    pub fn set_enable(&self, index: u32, enable: bool) {
        let register_index = (index / u32::BITS) as usize;
        let register_offset = index & (u32::BITS - 1);
        let register = if enable {
            Self::GICD_ISENABLER
        } else {
            Self::GICD_ICENABLER
        };
        self.write_register(
            register + register_index,
            self.read_register(register + register_index) | (1 << register_offset),
        );
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
    pub const DEFAULT_PRIORITY: u8 = 0xff;
    pub const DEFAULT_BINARY_POINT: u8 = 0x00;

    const ICC_SRE_SRE: u64 = 1;
    const ICC_IGRPEN1_EN: u64 = 1;
    const ICC_IGRPEN0_EN: u64 = 1;

    const GICR_CTLR: usize = 0x00;
    #[allow(dead_code)]
    const GICR_CTLR_UWP: u32 = 1 << 31;
    const GICR_CTLR_RWP: u32 = 1 << 3;
    const GICR_CTLR_ENABLE_LPIS: u32 = 1 << 0;

    //const GICR_TYPER: usize = 0x08;
    const GICR_TYPER_AFFINITY: usize = 0x0C;

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

    fn init(&mut self) -> bool {
        unsafe { cpu::set_icc_sre(cpu::get_icc_sre() | Self::ICC_SRE_SRE) };
        if (unsafe { cpu::get_icc_sre() } & Self::ICC_SRE_SRE) == 0 {
            pr_err!("GICv3 or later with System Registers is disabled.");
            return false;
        }
        self.wait_rwp();
        self.write_register(Self::GICR_CTLR, Self::GICR_CTLR_ENABLE_LPIS);
        self.write_register(
            Self::GICR_WAKER,
            self.read_register(Self::GICR_WAKER) & !Self::GCIR_WAKER_PROCESSOR_SLEEP,
        );
        while (self.read_register(Self::GICR_WAKER) & Self::GICR_WAKER_CHILDREN_ASLEEP) != 0 {
            core::hint::spin_loop();
        }
        self.set_priority_mask(Self::DEFAULT_PRIORITY);
        self.set_binary_point(Self::DEFAULT_BINARY_POINT);
        unsafe { cpu::set_icc_igrpen1(Self::ICC_IGRPEN1_EN) };
        unsafe { cpu::set_icc_igrpen0(Self::ICC_IGRPEN0_EN) };
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

    pub fn set_binary_point(&self, point: u8) {
        unsafe {
            cpu::set_icc_bpr0(point as u64);
            cpu::set_icc_bpr1(point as u64);
        }
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
            (self.read_register(Self::GICR_IGROUPR0) & !(1 << index)) | ((data) << index),
        );

        let data = match group {
            GicV3Group::NonSecureEl1 => 0,
        };
        self.write_register(
            Self::GICR_IGRPMODR0,
            (self.read_register(Self::GICR_IGRPMODR0) & !(1 << index)) | ((data) << index),
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
