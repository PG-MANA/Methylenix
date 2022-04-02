//!
//! AArch64 Virtual Memory Paging Table Entry
//!
//! Supported 48bit-OA, Level 0~3

use crate::arch::target_arch::paging::PAGE_MASK;
use crate::kernel::memory_manager::data_type::{Address, MemoryPermissionFlags, PAddress};

pub const NUM_OF_TOP_LEVEL_TABLE_ENTRIES: usize = 512;
pub const NUM_OF_TABLE_ENTRIES: usize = 512;

#[derive(Clone)]
pub struct TableEntry(u64);

impl TableEntry {
    const TABLE_ADDRESS_MASK: u64 = ((1 << 52) - 1) & (PAGE_MASK as u64);
    const OUTPUT_ADDRESS_MASK: u64 = ((1 << 50) - 1) & (PAGE_MASK as u64);
    const XN_OFFSET: u64 = 54;
    const XN: u64 = 1 << Self::XN_OFFSET;
    const AF_OFFSET: u64 = 10;
    const AF: u64 = 1 << Self::AF_OFFSET;
    const SH_OFFSET: u64 = 8;
    const SH: u64 = 0b11 << Self::SH_OFFSET;
    const AP_OFFSET: u64 = 6;
    const AP: u64 = 0b11 << Self::AP_OFFSET;
    const ATTR_INDEX_OFFSET: u64 = 2;
    const ATTR_INDEX: u64 = 0b111 << Self::ATTR_INDEX_OFFSET;

    pub const fn new() -> Self {
        Self(0)
    }

    pub fn init(&mut self) {
        *self = Self::new();
    }

    pub const fn create_table_entry(table_address: PAddress) -> Self {
        Self((table_address.to_usize() as u64) | 0b11)
    }

    pub fn invalidate(&mut self) {
        self.0 = 0;
    }

    pub fn validate_as_level3_descriptor(&mut self) {
        self.0 |= 0b11;
    }

    pub fn validate_as_block_descriptor(&mut self) {
        self.0 |= 0b01;
    }

    pub const fn is_validated(&self) -> bool {
        !((self.0 & 0b11) == 0b00)
    }

    pub const fn is_table_descriptor(&self) -> bool {
        (self.0 & 0b11) == 0b11
    }

    pub const fn is_block_descriptor(&self) -> bool {
        (self.0 & 0b11) == 0b01
    }

    pub const fn is_level3_descriptor(&self) -> bool {
        (self.0 & 0b11) == 0b11
    }

    pub const fn get_next_table_address(&self) -> PAddress {
        PAddress::new((self.0 & Self::TABLE_ADDRESS_MASK) as usize)
    }

    pub const fn get_output_address(&self) -> PAddress {
        PAddress::new((self.0 & Self::OUTPUT_ADDRESS_MASK) as usize)
    }

    pub fn set_output_address(&mut self, output_address: PAddress) {
        self.0 =
            (self.0 & !Self::OUTPUT_ADDRESS_MASK) | (output_address.to_usize() as u64) | Self::AF;
    }

    pub fn set_shareability(&mut self, shareability: u64) {
        self.0 = (self.0 & !Self::SH) | (shareability << Self::SH_OFFSET);
    }

    pub fn get_permission(&self) -> MemoryPermissionFlags {
        let xn = (self.0 & Self::XN) != 0;
        let p = (self.0 & Self::AP) >> Self::AP_OFFSET;
        MemoryPermissionFlags::new(true, (p & (1 << 1)) == 0, !xn, (p & 1) != 0)
    }

    pub fn set_permission(&mut self, permission: MemoryPermissionFlags) {
        self.0 = (self.0 & !(Self::AP | Self::XN))
            | (((!permission.is_executable()) as u64) << Self::XN_OFFSET)
            | (((((!permission.is_writable()) as u64) << 1)
                | (permission.is_user_accessible()) as u64)
                << Self::AP_OFFSET);
    }

    pub const fn get_memory_attribute_index(&self) -> u64 {
        (self.0 & Self::ATTR_INDEX) >> Self::ATTR_INDEX_OFFSET
    }

    pub fn set_memory_attribute_index(&mut self, index: u64) {
        self.0 = (self.0 & !Self::ATTR_INDEX) | (index << Self::ATTR_INDEX_OFFSET);
    }
}
