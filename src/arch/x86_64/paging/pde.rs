/*
 * Page Directory Entry
 */

use super::PAGE_MASK;

pub const PDE_MAX_ENTRY: usize = 512;

//PDEの53bit目はPTEの配列がセットされているかどうかの確認に利用している。

pub struct PDE {
    flags: u64,
}

impl PDE {
    #![allow(dead_code)]
    pub const fn new() -> PDE {
        PDE { flags: 0 }
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

    pub fn is_pte_set(&self) -> bool {
        self.get_bit(1 << 52)
    }

    pub fn set_pte_set(&mut self, b: bool) {
        self.set_bit(1 << 52, b);
    }

    pub fn is_present(&self) -> bool {
        self.get_bit(1 << 0)
    }

    pub fn set_present(&mut self, b: bool) {
        self.set_bit(1 << 0, b);
    }

    pub fn set_writable(&mut self, b: bool) {
        self.set_bit(1 << 1, b);
    }

    pub fn set_user_accessible(&mut self, b: bool) {
        self.set_bit(1 << 2, b);
    }

    pub fn set_wtc(&mut self, b: bool) {
        //write through caching
        self.set_bit(1 << 3, b);
    }

    pub fn set_disable_cache(&mut self, b: bool) {
        self.set_bit(1 << 4, b);
    }

    pub fn is_accessed(&self) -> bool {
        self.get_bit(1 << 5)
    }

    pub fn off_accessed(&mut self) {
        self.set_bit(1 << 5, false);
    }

    pub fn is_dirty(&self) -> bool {
        self.get_bit(1 << 6)
    }

    pub fn off_dirty(&mut self) {
        self.set_bit(1 << 6, false);
    }

    pub fn set_huge(&mut self, b: bool) {
        self.set_bit(1 << 7, b);
    }

    pub fn set_global(&mut self, b: bool) {
        self.set_bit(1 << 8, b);
    }

    pub fn set_no_excuse(&mut self, b: bool) {
        self.set_bit(1 << 63, b);
    }

    pub fn get_addr(&self) -> Option<usize> {
        if self.is_pte_set() {
            Some((self.flags & 0x000FFFFF_FFFFF000) as usize)
        } else {
            None
        }
    }

    pub fn set_addr(&mut self, address: usize) -> bool {
        if (address & !PAGE_MASK) == 0 {
            self.set_bit((0x000FFFFF_FFFFF000 & address) as u64, true);
            self.set_pte_set(true);
            true
        } else {
            false
        }
    }
}
