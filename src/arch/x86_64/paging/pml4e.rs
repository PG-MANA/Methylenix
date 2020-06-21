/*
 * Page Map Level 4 Entry
 */

use super::PagingEntry;
use super::PAGE_MASK;

pub const PML4_MAX_ENTRY: usize = 512;

/* PML4Eの53bit目はPDPTがセットされているかどうかの確認に利用している。 */

pub struct PML4E {
    flags: u64,
}

impl PML4E {
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

    pub fn set_huge(&mut self, b: bool) {
        self.set_bit(1 << 7, b);
    }
}

impl PagingEntry for PML4E {
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

    fn get_address(&self) -> Option<usize> {
        if self.is_address_set() {
            Some((self.flags & 0x000FFFFF_FFFFF000) as usize)
        } else {
            None
        }
    }

    fn set_address(&mut self, address: usize) -> bool {
        if (address & !PAGE_MASK) == 0 {
            self.set_bit((0x000FFFFF_FFFFF000 & address) as u64, true);
            self.set_address_set(true);
            true
        } else {
            false
        }
    }
}
