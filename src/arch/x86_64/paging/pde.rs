/*
 * Copyright 2017 PG_MANA
 *
 * This software is Licensed under the Apache License Version 2.0
 * See LICENSE.md
 *
 * ページング実装(Page Directory Entries)
 */

//use
use paging::pt::PageTable;
use paging::pte::PTE;

pub struct PDE {
    flags: u64,
}

impl PDE {
    /*pub fn new(/*規格未定*/) -> PTE{

    }*/

    pub fn new_empty() -> PTE {
        PTE { flags: 0 }
    }

    fn set_bit(&mut self, addr: u64, b: bool) {
        if b {
            self.flags |= addr;
        } else {
            self.flags &= !addr;
        }
    }

    fn get_bit(&self, addr: u64) -> bool {
        if (self.flags & addr) == 0 {
            false
        } else {
            true
        }
    }

    pub fn present(&self) -> bool {
        self.get_bit(1 << 0)
    }

    pub fn setPresent(&mut self, b: bool) {
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

    pub fn accessed(&self) -> bool {
        self.get_bit(1 << 5)
    }

    pub fn off_accessed(&mut self) {
        self.set_bit(1 << 5, false);
    }

    pub fn dirty(&self) -> bool {
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

    pub fn addr(&self) -> Option<usize> {
        if self.present() {
            Some((self.flags & 0xFFFFFFFFFF000) as usize)
        } else {
            None
        }
    }

    pub fn page(&self) -> Option<Frame> {
        let ad = self.addr();
        if ad.is_none() {
            None
        } else {
            Some(Frame::makefromaddr(self.addr().unwrap()))
        } //もっと短くかけないかな
    }

    pub fn set_addr(&mut self, page: &Frame) {
        assert!(page.startAddr() & !0xFFFFFFFFFF000 == 0);
        self.flags &= (!(0xFFFFFFFFFF000) | page.startAddr()) as u64;
    }

    pub fn set_no_excuse(&mut self, b: bool) {
        self.set_bit(1 << 63, b);
    }
}
