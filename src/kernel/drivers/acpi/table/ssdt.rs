//!
//! Secondary System Description Table
//!
//! This manager contains the information of SSDT
//! Definition block is treated by AML module.
//!

use super::super::INITIAL_MMAP_SIZE;

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{Address, MSize, VAddress};

#[repr(C, packed)]
struct SSDT {
    signature: [u8; 4],
    length: u32,
    major_version: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: [u8; 4],
    creator_revision: u32,
}

pub struct SsdtManager {
    base_address: VAddress,
}

impl SsdtManager {
    pub const SIGNATURE: [u8; 4] = *b"SSDT";
    pub const fn new() -> Self {
        Self {
            base_address: VAddress::new(0),
        }
    }

    pub const fn is_initialized(&self) -> bool {
        !self.base_address.is_zero()
    }

    pub fn init(&mut self, ssdt_vm_address: VAddress) -> bool {
        /* ssdt_vm_address must be accessible */
        let ssdt = unsafe { &*(ssdt_vm_address.to_usize() as *const SSDT) };
        if ssdt.major_version > 2 {
            pr_err!("Not supported SSDT version:{}", ssdt.major_version);
        }

        let ssdt_vm_address = if let Ok(a) = get_kernel_manager_cluster()
            .memory_manager
            .lock()
            .unwrap()
            .mremap_dev(
                ssdt_vm_address,
                INITIAL_MMAP_SIZE.into(),
                (ssdt.length as usize).into(),
            ) {
            a
        } else {
            pr_err!("Cannot map memory area of SSDT.");
            return false;
        };
        self.base_address = ssdt_vm_address;
        return true;
    }

    pub const fn get_definition_block_address_and_size(&self) -> (VAddress, MSize) {
        let dsdt = unsafe { &*(self.base_address.to_usize() as *const SSDT) };
        (
            self.base_address + MSize::new(core::mem::size_of::<SSDT>()),
            MSize::new(dsdt.length as usize - core::mem::size_of::<SSDT>()),
        )
    }
}
