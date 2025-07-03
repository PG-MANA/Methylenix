//!
//! Differentiated System Description Table
//!
//! This manager contains the information of DSDT
//! Definition block is treated by AML module.
//!

use super::AcpiTable;

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

impl AcpiTable for DsdtManager {
    const SIGNATURE: [u8; 4] = *b"DSDT";

    fn new() -> Self {
        Self {
            base_address: VAddress::new(0),
        }
    }

    fn init(&mut self, vm_address: VAddress) -> Result<(), ()> {
        /* dsdt_vm_address must be accessible */
        let dsdt = unsafe { &*(vm_address.to_usize() as *const DSDT) };
        if dsdt.major_version > 2 {
            pr_err!("Not supported DSDT version:{}", dsdt.major_version);
        }

        let dsdt_vm_address = remap_table!(vm_address, dsdt.length);
        self.base_address = dsdt_vm_address;
        Ok(())
    }
}

impl DsdtManager {
    pub const fn is_initialized(&self) -> bool {
        !self.base_address.is_zero()
    }

    pub const fn get_definition_block_address_and_size(&self) -> (VAddress, MSize) {
        let dsdt = unsafe { &*(self.base_address.to_usize() as *const DSDT) };
        (
            self.base_address + MSize::new(size_of::<DSDT>()),
            MSize::new(dsdt.length as usize - size_of::<DSDT>()),
        )
    }
}
