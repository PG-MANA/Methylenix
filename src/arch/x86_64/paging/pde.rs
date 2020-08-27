//!
//! Page Directory Entry
//!
//! See PageManager for the detail.

use super::PagingEntry;
use super::PAGE_MASK;

use kernel::memory_manager::data_type::{Address, PAddress};

pub const PD_MAX_ENTRY: usize = 512;

/* 53th bit(1 << 52) of PDE is used to check if the address is valid. */

pub struct PDE {
    flags: u64,
}

impl PDE {
    #![allow(dead_code)]
    pub const fn new() -> Self {
        Self { flags: 0 }
    }

    pub fn init(&mut self) {
        self.flags = 0;
        self.set_user_accessible(true);
        self.set_writable(true);
    }

    fn set_bit(&mut self, bit: u64, b: bool) {
        if b {
            self.flags |= bit;
        } else {
            self.flags &= !bit;
        }
    }

    fn get_bit(&self, bit: u64) -> bool {
        if (self.flags & bit) == 0 {
            false
        } else {
            true
        }
    }

    pub fn is_address_set(&self) -> bool {
        self.get_bit(1 << 52)
    }

    pub fn set_address_set(&mut self, b: bool) {
        self.set_bit(1 << 52, b);
    }

    pub fn is_huge(&self) -> bool {
        self.get_bit(1 << 7)
    }

    pub fn set_huge(&mut self, b: bool) {
        assert_eq!(self.is_present(), false);
        self.set_bit(1 << 7, b);
    }
}

impl PagingEntry for PDE {
    fn is_present(&self) -> bool {
        self.get_bit(1 << 0)
    }

    fn set_present(&mut self, b: bool) {
        self.set_bit(1 << 0, b);
    }

    fn is_writable(&self) -> bool {
        self.get_bit(1 << 1)
    }

    fn set_writable(&mut self, b: bool) {
        self.set_bit(1 << 1, b);
    }

    fn is_user_accessible(&self) -> bool {
        self.get_bit(1 << 2)
    }

    fn set_user_accessible(&mut self, b: bool) {
        self.set_bit(1 << 2, b);
    }

    fn set_wtc(&mut self, b: bool) {
        //write through caching
        self.set_bit(1 << 3, b);
    }

    fn set_disable_cache(&mut self, b: bool) {
        self.set_bit(1 << 4, b);
    }

    fn is_accessed(&self) -> bool {
        self.get_bit(1 << 5)
    }

    fn off_accessed(&mut self) {
        self.set_bit(1 << 5, false);
    }

    fn is_dirty(&self) -> bool {
        self.get_bit(1 << 6)
    }

    fn off_dirty(&mut self) {
        self.set_bit(1 << 6, false);
    }

    fn set_global(&mut self, b: bool) {
        self.set_bit(1 << 8, b);
    }

    fn is_no_execute(&self) -> bool {
        self.get_bit(1 << 63)
    }

    fn set_no_execute(&mut self, b: bool) {
        self.set_bit(1 << 63, b);
    }

    fn get_address(&self) -> Option<PAddress> {
        if self.is_address_set() {
            if self.is_huge() {
                Some(((self.flags & 0x000FFFFF_FFFF0000) as usize).into())
            } else {
                Some(((self.flags & 0x000FFFFF_FFFFF000) as usize).into())
            }
        } else {
            None
        }
    }

    fn set_address(&mut self, address: PAddress) -> bool {
        if (address.to_usize() & !PAGE_MASK) == 0 {
            if self.is_huge() {
                self.set_bit((0x000FFFFF_FFFF0000 & address.to_usize()) as u64, true);
            } else {
                self.set_bit((0x000FFFFF_FFFFF000 & address.to_usize()) as u64, true);
            }
            self.set_address_set(true);
            true
        } else {
            false
        }
    }
}
