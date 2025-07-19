//!
//! Generic Interrupt Controller version 3 and 4
//!

use super::super::InterruptGroup;

use crate::arch::target_arch::device::cpu;

use crate::kernel::memory_manager::{
    data_type::{Address, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress, VAddress},
    io_remap,
};

const GIC_V3_DISTRIBUTOR_MEMORY_MAP_SIZE: MSize = MSize::new(0x10000);

const GIC_V3_REDISTRIBUTOR_MEMORY_MAP_SIZE: MSize = MSize::new(0x20000);
const GIC_V4_REDISTRIBUTOR_MEMORY_MAP_SIZE: MSize = MSize::new(0x40000);

pub struct GicV3Distributor {
    interrupt_distributor_physical_address: PAddress,
    /* For MSI */
    interrupt_distributor_base_address: VAddress,
    interrupt_redistributor_discovery_base_address: Option<VAddress>,
    interrupt_redistributor_discovery_length: u32,
    version: u8,
}

pub struct GicV3Redistributor {
    base_address: VAddress,
}

impl GicV3Distributor {
    const GICD_CTLR: usize = 0x00;
    const GCID_CTLR_RWP: u32 = 1 << 31;
    //const GCID_CTLR_DS: u32 = 1 << 6;
    const GICD_CTLR_ARE: u32 = 1 << 5;
    const GCID_CTLR_ENABLE_GRP1NS: u32 = 1 << 1;
    const GCID_CTLR_ENABLE_GRP0: u32 = 1;
    const GICD_IGROUPR: usize = 0x0080;
    const GICD_ISENABLER: usize = 0x0100;
    const GICD_ICENABLER: usize = 0x0180;
    const GICD_ISPENDR: usize = 0x0200;
    const GICD_IPRIORITYR: usize = 0x0400;
    const GICD_ICFGR: usize = 0x0C00;
    const GICD_IGRPMODR: usize = 0x0D00;
    const GICD_IROUTER: usize = 0x6100;

    pub fn new(
        base_address: PAddress,
        version: u8,
        redistributor_range: Option<(PAddress, MSize)>,
    ) -> Result<Self, ()> {
        let interrupt_distributor_base_address = match io_remap!(
            base_address,
            GIC_V3_DISTRIBUTOR_MEMORY_MAP_SIZE,
            MemoryPermissionFlags::data(),
            MemoryOptionFlags::DEVICE_MEMORY
        ) {
            Ok(v) => v,
            Err(e) => {
                pr_err!("Failed to map Generic Interrupt Distributor area: {:?}", e);
                return Err(());
            }
        };
        let interrupt_redistributor_discovery_base_address;
        let interrupt_redistributor_discovery_length;
        if let Some((base, size)) = redistributor_range {
            interrupt_redistributor_discovery_base_address = match io_remap!(
                base,
                size,
                MemoryPermissionFlags::data(),
                MemoryOptionFlags::DEVICE_MEMORY
            ) {
                Ok(v) => Some(v),
                Err(e) => {
                    pr_err!(
                        "Failed to map Generic Interrupt Redistributor area: {:?}",
                        e
                    );
                    return Err(());
                }
            };
            interrupt_redistributor_discovery_length = size.to_usize() as u32;
        } else {
            interrupt_redistributor_discovery_base_address = None;
            interrupt_redistributor_discovery_length = 0;
        }

        Ok(Self {
            interrupt_distributor_physical_address: base_address,
            interrupt_distributor_base_address,
            interrupt_redistributor_discovery_base_address,
            interrupt_redistributor_discovery_length,
            version,
        })
    }

    pub fn init(&mut self) -> bool {
        self.write_register(Self::GICD_CTLR, Self::GICD_CTLR_ARE);
        self.wait_rwp();
        self.write_register(
            Self::GICD_CTLR,
            Self::GICD_CTLR_ARE | Self::GCID_CTLR_ENABLE_GRP1NS | Self::GCID_CTLR_ENABLE_GRP0,
        );
        true
    }

    pub fn init_redistributor<F>(&self, redistributor_info: F) -> Option<GicV3Redistributor>
    where
        F: FnOnce(u64) -> Option<PAddress>,
    {
        let redistributor_address =
            self.find_redistributor_address_of_this_pe(redistributor_info)?;
        let mut redistributor_manager = GicV3Redistributor::new(redistributor_address);
        if !redistributor_manager.init() {
            pr_err!("Failed to init GIC Redistributor.");
            return None;
        }
        Some(redistributor_manager)
    }

    fn find_redistributor_address_of_this_pe<F>(&self, finder: F) -> Option<VAddress>
    where
        F: FnOnce(u64) -> Option<PAddress>,
    {
        if let Some(discovery_base) = self.interrupt_redistributor_discovery_base_address {
            let mut pointer = 0;
            let redistributor_memory_size = if self.version == 3 {
                GIC_V3_REDISTRIBUTOR_MEMORY_MAP_SIZE.to_usize()
            } else if self.version == 4 {
                GIC_V4_REDISTRIBUTOR_MEMORY_MAP_SIZE.to_usize()
            } else {
                unreachable!()
            };
            let self_affinity = cpu::mpidr_to_packed_affinity(cpu::get_mpidr());
            while pointer < (self.interrupt_redistributor_discovery_length as usize) {
                let base = discovery_base.to_usize() + pointer;
                if unsafe { *((base + GicV3Redistributor::GICR_TYPER_AFFINITY) as *const u32) }
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
            None
        } else if let Some(base) = finder(cpu::mpidr_to_affinity(cpu::get_mpidr())) {
            match io_remap!(
                base,
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
        let data = match group {
            InterruptGroup::NonSecureEl1 => 0,
        };
        self.write_register(
            Self::GICD_IGRPMODR + register_index,
            (self.read_register(Self::GICD_IGRPMODR + register_index) & !(1 << register_offset))
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

    pub fn set_routing_to_this(&self, interrupt_id: u32, is_routing_mode: bool) {
        if is_routing_mode {
            unimplemented!()
        } else {
            unsafe {
                core::ptr::write_volatile(
                    (self.interrupt_distributor_base_address.to_usize()
                        + Self::GICD_IROUTER
                        + (interrupt_id as usize) * size_of::<u64>())
                        as *mut u64,
                    cpu::mpidr_to_affinity(cpu::get_mpidr()),
                )
            }
        }
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
        /* cpu_id is mpidr */
        let affinity_0 = (cpu_id & 0xff) as u64;
        let affinity_1 = ((cpu_id >> 8) & 0xff) as u64;
        let affinity_2 = ((cpu_id >> 16) & 0xff) as u64;
        let affinity_3 = ((cpu_id >> 32) & 0xff) as u64;

        let icc_sgi1r = (affinity_3 << 48)
            | (affinity_2 << 32)
            | ((interrupt_id as u64) << 24)
            | (affinity_1 << 16)
            | affinity_0;
        unsafe { cpu::set_icc_sgi1r_el1(icc_sgi1r) };
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

impl GicV3Redistributor {
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

    const GICR_IGROUPR: usize = 0x10000 + 0x0080;
    const GICR_IPRIORITYR: usize = 0x10000 + 0x0400;
    const GICR_IGRPMODR: usize = 0x10000 + 0x0D00;
    const GICR_ISENABLER: usize = 0x10000 + 0x0100;
    const GICR_ICENABLER: usize = 0x10000 + 0x0180;
    const GICR_ICFGR: usize = 0x10000 + 0x0C00;

    fn new(base_address: VAddress) -> Self {
        Self { base_address }
    }

    fn init(&mut self) -> bool {
        unsafe { cpu::set_icc_sre(cpu::get_icc_sre() | Self::ICC_SRE_SRE) };
        if (cpu::get_icc_sre() & Self::ICC_SRE_SRE) == 0 {
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
        true
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
        let register_index = ((index >> 2) as usize) * size_of::<u32>();
        let register_offset = (index & 0b11) << 3;
        self.write_register(
            Self::GICR_IPRIORITYR + register_index,
            (self.read_register(Self::GICR_IPRIORITYR + register_index)
                & !(0xFF << register_offset))
                | ((priority as u32) << register_offset),
        );
    }

    pub fn set_group(&self, index: u32, group: InterruptGroup) {
        if index > 31 {
            pr_err!("Invalid index: {:#X}", index);
            return;
        }

        let data = match group {
            InterruptGroup::NonSecureEl1 => 1,
        };
        self.write_register(
            Self::GICR_IGROUPR,
            (self.read_register(Self::GICR_IGROUPR) & !(1 << index)) | ((data) << index),
        );

        let data = match group {
            InterruptGroup::NonSecureEl1 => 0,
        };
        self.write_register(
            Self::GICR_IGRPMODR,
            (self.read_register(Self::GICR_IGRPMODR) & !(1 << index)) | ((data) << index),
        );
    }

    pub fn set_enable(&self, index: u32, to_enable: bool) {
        if index > 31 {
            pr_err!("Invalid index: {:#X}", index);
            return;
        }

        let register = if to_enable {
            Self::GICR_ISENABLER
        } else {
            Self::GICR_ICENABLER
        };

        self.write_register(register, 1 << index);
    }

    pub fn set_trigger_mode(&self, index: u32, is_level_trigger: bool) {
        if index > 31 {
            pr_err!("Invalid index: {:#X}", index);
            return;
        }
        let register_index = ((index / (u32::BITS / 2)) as usize) * size_of::<u32>();
        let register_offset = index & (u32::BITS / 2 - 1);

        self.write_register(
            Self::GICR_ICFGR + register_index,
            (self.read_register(Self::GICR_ICFGR + register_index) & !(0x03 << register_offset))
                | ((((!is_level_trigger) as u32) << 1) << register_offset),
        );
    }

    /*pub fn get_highest_priority_pending_interrupt(&self) -> (u32, GicV3Group) {
        (
            unsafe { cpu::get_icc_hppir1() as u32 },
            GicV3Group::NonSecureEl1,
        )
    }*/

    pub fn get_acknowledge(&self) -> (u32, InterruptGroup) {
        (cpu::get_icc_iar1() as u32, InterruptGroup::NonSecureEl1)
    }

    pub fn send_eoi(&self, index: u32, group: InterruptGroup) {
        match group {
            InterruptGroup::NonSecureEl1 => {
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
