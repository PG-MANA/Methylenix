//!
//! Secondary System Description Table
//!
//! This manager contains the information of SSDT
//! Definition block is treated by AML module.
//!

use super::AcpiTable;

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

impl AcpiTable for SsdtManager {
    const SIGNATURE: [u8; 4] = *b"SSDT";
    fn new() -> Self {
        Self {
            base_address: VAddress::new(0),
        }
    }

    fn init(&mut self, vm_address: VAddress) -> Result<(), ()> {
        /* ssdt_vm_address must be accessible */
        let ssdt = unsafe { &*(vm_address.to_usize() as *const SSDT) };
        if ssdt.major_version > 2 {
            pr_err!("Not supported SSDT version:{}", ssdt.major_version);
        }

        let ssdt_vm_address = remap_table!(vm_address, ssdt.length);
        self.base_address = ssdt_vm_address;
        Ok(())
    }
}

impl SsdtManager {
    pub const fn is_initialized(&self) -> bool {
        !self.base_address.is_zero()
    }

    pub const fn get_definition_block_address_and_size(&self) -> (VAddress, MSize) {
        let dsdt = unsafe { &*(self.base_address.to_usize() as *const SSDT) };
        (
            self.base_address + MSize::new(size_of::<SSDT>()),
            MSize::new(dsdt.length as usize - size_of::<SSDT>()),
        )
    }
}
