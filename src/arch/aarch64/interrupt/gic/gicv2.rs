//!
//! Generic Interrupt Controller version 2
//!

use super::super::InterruptGroup;

use crate::arch::target_arch::device::cpu;

use crate::kernel::manager_cluster::get_cpu_manager_cluster;
use crate::kernel::memory_manager::data_type::{
    Address, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress, VAddress,
};
use crate::kernel::memory_manager::io_remap;

const GIC_V2_DISTRIBUTOR_MEMORY_MAP_SIZE: MSize = MSize::new(0x1000);
const GIC_V2_REDISTRIBUTOR_MEMORY_MAP_SIZE: MSize = MSize::new(0x2000); /* Actually, 0x1008 */

pub struct GicV2Distributor {
    interrupt_distributor_physical_address: PAddress,
    /* For MSI */
    interrupt_distributor_base_address: VAddress,
}

pub struct GicV2Redistributor {
    base_address: VAddress,
    distributor: *const GicV2Distributor,
}

impl GicV2Distributor {
    const GICD_CTLR: usize = 0x000;
    const GCID_CTLR_ENABLE_GRP1NS: u32 = 1 << 1;
    const GCID_CTLR_ENABLE_GRP0: u32 = 1;
    const GICD_IGROUPR: usize = 0x080;
    const GICD_ISENABLER: usize = 0x100;
    const GICD_ICENABLER: usize = 0x180;
    const GICD_ISPENDR: usize = 0x200;
    const GICD_IPRIORITYR: usize = 0x400;
    const GICD_ITARGETSR: usize = 0x800;
    const GICD_ICFGR: usize = 0xC00;
    const GICD_SGIR: usize = 0xF00;

    pub fn new(base_address: PAddress) -> Result<Self, ()> {
        let interrupt_distributor_base_address = match io_remap!(
            base_address,
            GIC_V2_DISTRIBUTOR_MEMORY_MAP_SIZE,
            MemoryPermissionFlags::data(),
            MemoryOptionFlags::DEVICE_MEMORY
        ) {
            Ok(v) => v,
            Err(e) => {
                pr_err!("Failed to map Generic Interrupt Distributor area: {:?}", e);
                return Err(());
            }
        };

        Ok(Self {
            interrupt_distributor_physical_address: base_address,
            interrupt_distributor_base_address,
        })
    }

    pub fn init(&self) -> bool {
        self.write_register(
            Self::GICD_CTLR,
            Self::GCID_CTLR_ENABLE_GRP1NS | Self::GCID_CTLR_ENABLE_GRP0,
        );
        true
    }

    pub fn init_redistributor<F>(&self, redistributor_info: F) -> Option<GicV2Redistributor>
    where
        F: FnOnce(u64) -> Option<(PAddress, u8)>,
    {
        let redistributor_address =
            self.find_redistributor_address_of_this_pe(redistributor_info)?;
        let mut redistributor_manager = GicV2Redistributor::new(redistributor_address, self);
        if !redistributor_manager.init() {
            pr_err!("Failed to init GIC Redistributor.");
            return None;
        }
        Some(redistributor_manager)
    }

    fn find_redistributor_address_of_this_pe<F>(&self, finder: F) -> Option<VAddress>
    where
        F: FnOnce(u64) -> Option<(PAddress, u8)>,
    {
        if let Some((base, cpu_interface_number)) = finder(cpu::mpidr_to_affinity(cpu::get_mpidr()))
        {
            get_cpu_manager_cluster()
                .arch_depend_data
                .cpu_interface_number = cpu_interface_number;
            match io_remap!(
                base,
                GIC_V2_REDISTRIBUTOR_MEMORY_MAP_SIZE,
                MemoryPermissionFlags::data(),
                MemoryOptionFlags::DEVICE_MEMORY
            ) {
                Ok(v) => Some(v),
                Err(e) => {
                    pr_err!("Failed to map Redistributor: {:?}", e);
                    None
                }
            }
        } else {
            pr_err!("No redistributor information is found.");
            None
        }
    }

    pub fn set_priority(&self, index: u32, priority: u8) {
        let register_index = ((index >> 2) as usize) * size_of::<u32>();
        let register_offset = (index & 0b11) << 3;
        self.write_register(
            Self::GICD_IPRIORITYR + register_index,
            (self.read_register(Self::GICD_IPRIORITYR + register_index)
                & !(0xFF << register_offset))
                | ((priority as u32) << register_offset),
        );
    }

    pub fn set_group(&self, index: u32, group: InterruptGroup) {
        let register_index = ((index / u32::BITS) as usize) * size_of::<u32>();
        let register_offset = index & (u32::BITS - 1);
        let data = match group {
            InterruptGroup::NonSecureEl1 => 1,
        };
        self.write_register(
            Self::GICD_IGROUPR + register_index,
            (self.read_register(Self::GICD_IGROUPR + register_index) & !(1 << register_offset))
                | (data << register_offset),
        );
    }

    pub fn set_enable(&self, index: u32, enable: bool) {
        let register_index = ((index / u32::BITS) as usize) * size_of::<u32>();
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

    pub fn set_trigger_mode(&self, index: u32, is_level_trigger: bool) {
        let register_index = ((index / (u32::BITS / 2)) as usize) * size_of::<u32>();
        let register_offset = index & (u32::BITS / 2 - 1);

        self.write_register(
            Self::GICD_ICFGR + register_index,
            (self.read_register(Self::GICD_ICFGR + register_index) & !(0x03 << register_offset))
                | ((((!is_level_trigger) as u32) << 1) << register_offset),
        );
    }

    pub fn set_routing_to_this(&self, interrupt_id: u32) {
        let cpu_id = get_cpu_manager_cluster()
            .arch_depend_data
            .cpu_interface_number;
        if cpu_id >= 8 {
            pr_err!("Invalid CPU interface {cpu_id}.");
            return;
        }
        let register_index = ((interrupt_id >> 2) as usize) * size_of::<u32>();
        let register_offset = (interrupt_id & 0b11) << 3;
        self.write_register(
            Self::GICD_ITARGETSR + register_index,
            (self.read_register(Self::GICD_ITARGETSR + register_index)
                & !(0xFF << register_offset))
                | ((1 << cpu_id) << register_offset),
        );
    }

    /// For MSI
    pub fn get_pending_register_address_and_data(&self, interrupt_id: u32) -> (PAddress, u8) {
        let register_index = (interrupt_id / u8::BITS) as usize;
        let register_offset = interrupt_id & (u8::BITS - 1);
        (
            self.interrupt_distributor_physical_address
                + MSize::new(Self::GICD_ISPENDR + register_index),
            1u8 << register_offset,
        )
    }

    pub fn send_sgi(&self, cpu_id: usize, interrupt_id: u32) {
        if cpu_id >= 8 {
            pr_err!("Invalid CPU interface {cpu_id}.");
            return;
        }
        let sgir = ((1 << cpu_id) << 16) | interrupt_id;
        self.write_register(Self::GICD_SGIR, sgir);
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

impl GicV2Redistributor {
    pub const DEFAULT_PRIORITY: u8 = 0xff;
    pub const DEFAULT_BINARY_POINT: u8 = 0x00;

    const GICC_CTLR: usize = 0x0000;
    const GICC_CTLR_ENABLE_GRP1: u32 = 1 << 0;

    const GICC_PMR: usize = 0x0004;
    const GICC_BPR: usize = 0x0008;
    const GICC_IAR: usize = 0x000C;
    const GICC_EOIR: usize = 0x0010;

    fn new(cpu_base_address: VAddress, distributor: *const GicV2Distributor) -> Self {
        Self {
            base_address: cpu_base_address,
            distributor,
        }
    }

    fn init(&mut self) -> bool {
        self.write_register(Self::GICC_CTLR, Self::GICC_CTLR_ENABLE_GRP1);
        self.set_priority_mask(Self::DEFAULT_PRIORITY);
        self.set_binary_point(Self::DEFAULT_BINARY_POINT);
        true
    }

    pub const fn is_available(&self) -> bool {
        !self.base_address.is_zero()
    }

    /// Set Priority Mask
    ///
    /// If the priority of interrupt request  is higher(nearer 0), this processing element will generate interrupt.
    pub fn set_priority_mask(&self, mask: u8) {
        self.write_register(Self::GICC_PMR, mask as u32);
    }

    pub fn set_binary_point(&self, point: u8) {
        self.write_register(Self::GICC_BPR, point as u32);
    }

    pub fn set_priority(&self, index: u32, priority: u8) {
        if index > 31 {
            pr_err!("Invalid index: {:#X}", index);
            return;
        }
        unsafe { &*self.distributor }.set_priority(index, priority);
    }

    pub fn set_group(&self, index: u32, group: InterruptGroup) {
        if index > 31 {
            pr_err!("Invalid index: {:#X}", index);
            return;
        }
        unsafe { &*self.distributor }.set_group(index, group);
    }

    pub fn set_enable(&self, index: u32, to_enable: bool) {
        if index > 31 {
            pr_err!("Invalid index: {:#X}", index);
            return;
        }
        unsafe { &*self.distributor }.set_enable(index, to_enable);
    }

    pub fn set_trigger_mode(&self, index: u32, is_level_trigger: bool) {
        if index > 31 {
            pr_err!("Invalid index: {:#X}", index);
            return;
        }
        unsafe { &*self.distributor }.set_trigger_mode(index, is_level_trigger);
    }

    pub fn get_acknowledge(&self) -> (u32, InterruptGroup) {
        (
            self.read_register(Self::GICC_IAR),
            InterruptGroup::NonSecureEl1,
        )
    }

    pub fn send_eoi(&self, index: u32, group: InterruptGroup) {
        match group {
            InterruptGroup::NonSecureEl1 => {
                self.write_register(Self::GICC_EOIR, index);
            }
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
