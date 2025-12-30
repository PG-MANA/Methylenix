//!
//! Platform Level Interrupt Controller version 1.0
//!

use crate::arch::target_arch::interrupt::InterruptController;
use crate::kernel::memory_manager::{
    data_type::{Address, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress, VAddress},
    io_remap,
};

pub struct PlatformLevelInterruptController {
    base_address: VAddress,
    physical_base_address: PAddress,
}

impl PlatformLevelInterruptController {
    const MEMORY_MAP_SIZE: MSize = MSize::new(0x4000000);
    const REGISTER_SIZE: usize = size_of::<u32>();
    const REGISTER_BITS: usize = u32::BITS as usize;
    const INTERRUPT_SOURCE_PRIORITY: usize = 0x0000;
    const INTERRUPT_PENDING: usize = 0x1000;
    const INTERRUPT_ENABLE: usize = 0x2000;
    const PRIORITY_THRESHOLD: usize = 0x200000;
    const INTERRUPT_CLAIM: usize = 0x200004;
    const INTERRUPT_COMPLETION: usize = 0x200004;

    pub fn new(base_address: PAddress) -> Result<Self, ()> {
        let mapped_base_address = match io_remap!(
            base_address,
            Self::MEMORY_MAP_SIZE,
            MemoryPermissionFlags::data(),
            MemoryOptionFlags::DEVICE_MEMORY
        ) {
            Ok(v) => v,
            Err(e) => {
                pr_err!(
                    "Failed to map Platform Level Interrupt Controller area: {:?}",
                    e
                );
                return Err(());
            }
        };

        Ok(Self {
            physical_base_address: base_address,
            base_address: mapped_base_address,
        })
    }

    pub fn init(&mut self) -> bool {
        true
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

impl InterruptController for PlatformLevelInterruptController {
    fn set_priority(&self, interrupt_id: u32, priority: u32) {
        if interrupt_id == 0 || interrupt_id > 1024 {
            pr_err!("Interrupt ID {interrupt_id} does not exist");
            return;
        }
        self.write_register(
            Self::INTERRUPT_SOURCE_PRIORITY + (interrupt_id as usize * Self::REGISTER_SIZE),
            priority,
        );
    }

    fn set_pending(&self, interrupt_id: u32, pending: bool) {
        if interrupt_id == 0 || interrupt_id > 1024 {
            pr_err!("Interrupt ID {interrupt_id} does not exist");
            return;
        }
        let register_index = ((interrupt_id / u32::BITS) as usize) * Self::REGISTER_SIZE;
        let register_offset = interrupt_id & (u32::BITS - 1);
        let original = self.read_register(Self::INTERRUPT_PENDING + register_index);
        if pending {
            self.write_register(
                Self::INTERRUPT_PENDING + register_index,
                original | register_offset,
            );
        } else {
            self.write_register(
                Self::INTERRUPT_PENDING + register_index,
                original & !register_offset,
            );
        }
    }

    fn set_enable(&self, interrupt_id: u32, context: u32, enable: bool) {
        if interrupt_id == 0 || interrupt_id > 1024 {
            pr_err!("Interrupt ID {interrupt_id} does not exist");
            return;
        }
        let context_offset = (context as usize) * (1024 / 8);
        let register_index = ((interrupt_id / u32::BITS) as usize) * size_of::<u32>();
        let register_offset = interrupt_id & (u32::BITS - 1);
        let original = self.read_register(Self::INTERRUPT_ENABLE + context_offset + register_index);
        if enable {
            self.write_register(
                Self::INTERRUPT_ENABLE + context_offset + register_index,
                original | register_offset,
            );
        } else {
            self.write_register(
                Self::INTERRUPT_ENABLE + context_offset + register_index,
                original & !register_offset,
            );
        }
    }

    fn set_priority_threshold(&self, context: u32, threshold: u32) {
        self.write_register(
            Self::PRIORITY_THRESHOLD + (context as usize) * 0x1000,
            threshold,
        );
    }

    fn claim_interrupt(&self, context: u32) -> u32 {
        self.read_register(Self::INTERRUPT_CLAIM + (context as usize) * 0x1000)
    }

    fn send_eoi(&self, context: u32, interrupt_id: u32) {
        self.write_register(
            Self::INTERRUPT_COMPLETION + (context as usize) * 0x1000,
            interrupt_id,
        );
    }

    fn get_pending_register_address_and_data(&self, interrupt_id: u32) -> (PAddress, u8) {
        if interrupt_id == 0 || interrupt_id > 1024 {
            pr_err!("Interrupt ID {interrupt_id} does not exist");
            return (PAddress::new(0), 0);
        }
        let register_index = ((interrupt_id / u32::BITS) as usize) * Self::REGISTER_SIZE;
        let register_offset = interrupt_id & (u32::BITS - 1);
        (
            self.physical_base_address + MSize::new(Self::INTERRUPT_PENDING + register_index),
            1u8 << register_offset,
        )
    }
}
