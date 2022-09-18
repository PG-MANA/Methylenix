//!
//! Generic Timer Description Table
//!

use super::{AcpiTable, OptionalAcpiTable};

use crate::kernel::memory_manager::data_type::{Address, VAddress};
use crate::kernel::memory_manager::free_pages;

#[repr(C, packed)]
struct GTDT {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: [u8; 4],
    creator_revision: u32,
    cnt_control_base_physical_address: u64,
    reserved: u32,
    secure_el1_timer_gsiv: u32,
    secure_el1_timer_flags: u32,
    non_secure_el1_timer_gsiv: u32,
    non_secure_el1_timer_flags: u32,
    virtual_el1_timer_gsiv: u32,
    virtual_el1_timer_flags: u32,
    el2_timer_gsiv: u32,
    el2_timer_flags: u32,
    cnt_read_base_physical_address: u64,
    platform_timer_count: u32,
    platform_timer_offset: u32,
    virtual_el2_timer_gsiv: u32,
    virtual_el2_timer_flags: u32,
}

pub struct GtdtManager {
    base_address: VAddress,
}

impl AcpiTable for GtdtManager {
    const SIGNATURE: [u8; 4] = *b"GTDT";

    fn new() -> Self {
        Self {
            base_address: VAddress::new(0),
        }
    }

    fn init(&mut self, vm_address: VAddress) -> Result<(), ()> {
        /* vm_address must be accessible */
        let gtdt = unsafe { &*(vm_address.to_usize() as *const GTDT) };
        if gtdt.revision > 3 {
            pr_err!("Not supported GTDT revision:{}", gtdt.revision);
        }
        self.base_address = remap_table!(vm_address, gtdt.length);

        return Ok(());
    }
}

impl OptionalAcpiTable for GtdtManager {}

impl GtdtManager {
    pub const ADDRESS_INVALID: u64 = u64::MAX;

    pub fn get_cnt_control_base(&self) -> Option<usize> {
        if self.base_address.is_zero() {
            return None;
        }
        let gtdt = unsafe { &*(self.base_address.to_usize() as *const GTDT) };
        if gtdt.cnt_control_base_physical_address == Self::ADDRESS_INVALID {
            None
        } else {
            Some(gtdt.cnt_control_base_physical_address as usize)
        }
    }

    pub fn get_non_secure_el1_gsiv(&self) -> u32 {
        let gtdt = unsafe { &*(self.base_address.to_usize() as *const GTDT) };
        gtdt.non_secure_el1_timer_gsiv
    }

    pub fn get_non_secure_el1_flags(&self) -> u32 {
        let gtdt = unsafe { &*(self.base_address.to_usize() as *const GTDT) };
        gtdt.non_secure_el1_timer_flags
    }

    pub fn delete_map(self) {
        let _ = free_pages!(self.base_address);
    }
}
