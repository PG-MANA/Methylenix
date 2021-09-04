//!
//! Differentiated System Description Table
//!
//! This manager contains the information of DSDT
//! Definition block is treated by AML module.
//!

use super::super::INITIAL_MMAP_SIZE;

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{Address, MSize, VAddress};

#[repr(C, packed)]
struct DSDT {
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

pub struct DsdtManager {
    base_address: VAddress,
}

impl DsdtManager {
    pub const SIGNATURE: [u8; 4] = *b"DSDT";
    pub const fn new() -> Self {
        Self {
            base_address: VAddress::new(0),
        }
    }

    pub const fn is_initialized(&self) -> bool {
        !self.base_address.is_zero()
    }

    pub fn init(&mut self, dsdt_vm_address: VAddress) -> bool {
        /* dsdt_vm_address must be accessible */
        let dsdt = unsafe { &*(dsdt_vm_address.to_usize() as *const DSDT) };
        if dsdt.major_version > 2 {
            pr_err!("Not supported DSDT version:{}", dsdt.major_version);
        }

        let dsdt_vm_address = remap_table!(dsdt_vm_address, dsdt.length);
        self.base_address = dsdt_vm_address;
        return true;
    }

    pub const fn get_definition_block_address_and_size(&self) -> (VAddress, MSize) {
        let dsdt = unsafe { &*(self.base_address.to_usize() as *const DSDT) };
        (
            self.base_address + MSize::new(core::mem::size_of::<DSDT>()),
            MSize::new(dsdt.length as usize - core::mem::size_of::<DSDT>()),
        )
    }
}
