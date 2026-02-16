//!
//! RV64 Virtual Memory Paging Table Entry
//!
//! Supported Sv39, Sv48

use super::{PAGE_MASK, PAGE_SHIFT};

use crate::kernel::memory_manager::data_type::{Address, MemoryPermissionFlags, PAddress};

pub const NUM_OF_TABLE_ENTRIES: usize = 512;

#[derive(Clone)]
pub struct PageTableEntry(u64);

impl PageTableEntry {
    const PPN_MASK: u64 = ((1 << 54) - 1) & !((1 << 10) - 1);
    const PPN_OFFSET: usize = 10;
    //const RSW_OFFSET: usize = 8;
    //const RSW: u64 = 0b11 << Self::RSW_OFFSET;
    const D: u64 = 1 << 7;
    const A: u64 = 1 << 6;
    const G: u64 = 1 << 5;
    const U: u64 = 1 << 4;
    const X: u64 = 1 << 3;
    const W: u64 = 1 << 2;
    const R: u64 = 1 << 1;
    const VALID: u64 = 1;

    pub const fn new() -> Self {
        Self(0)
    }

    pub fn init(&mut self) {
        *self = Self::new();
    }

    pub fn create_table_entry(table_address: PAddress) -> Self {
        let mut e = Self::new();
        e.set_output_address(table_address);
        e.validate();
        e
    }

    pub const fn is_valid(&self) -> bool {
        self.0 & Self::VALID != 0
    }

    pub fn validate(&mut self) {
        self.0 |= Self::VALID;
    }

    pub fn invalidate(&mut self) {
        self.0 &= !Self::VALID;
    }

    pub const fn is_leaf(&self) -> bool {
        self.is_valid() && (self.0 & (Self::R | Self::W | Self::X) != 0)
    }

    pub const fn has_next(&self) -> bool {
        self.is_valid() && (self.0 & (Self::R | Self::W | Self::X) == 0)
    }

    pub const fn get_next_table_address(&self) -> PAddress {
        PAddress::new((((self.0 & Self::PPN_MASK) >> Self::PPN_OFFSET) << PAGE_SHIFT) as usize)
    }

    pub const fn get_output_address(&self) -> PAddress {
        PAddress::new((((self.0 & Self::PPN_MASK) >> Self::PPN_OFFSET) << PAGE_SHIFT) as usize)
    }

    pub fn set_output_address(&mut self, output_address: PAddress) {
        assert_eq!(output_address.to_usize() & !PAGE_MASK, 0);
        self.0 = (self.0 & !Self::PPN_MASK)
            | (((output_address.to_usize() as u64) >> PAGE_SHIFT) << Self::PPN_OFFSET);
    }

    pub const fn set_dirty_flag(&mut self) {
        self.0 |= Self::D;
    }

    pub const fn set_access_flag(&mut self) {
        self.0 |= Self::A;
    }

    pub const fn set_global_flag(&mut self) {
        self.0 |= Self::G;
    }

    pub const fn get_permission(&self) -> MemoryPermissionFlags {
        let r = (self.0 & Self::R) != 0;
        let w = (self.0 & Self::W) != 0;
        let x = (self.0 & Self::X) != 0;
        let u = (self.0 & Self::U) != 0;
        MemoryPermissionFlags::new(r, w, x, u)
    }

    pub const fn set_permission(&mut self, permission: MemoryPermissionFlags) {
        self.0 &= !(Self::R | Self::W | Self::X | Self::U);
        if permission.is_readable() {
            self.0 |= Self::R;
        }
        if permission.is_writable() {
            self.0 |= Self::W;
        }
        if permission.is_executable() {
            self.0 |= Self::X;
        }
        if permission.is_user_accessible() {
            self.0 |= Self::U;
        }
    }
}
