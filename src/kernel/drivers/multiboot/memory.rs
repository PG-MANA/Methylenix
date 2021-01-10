//!
//! Multiboot Memory Map Information
//!

use core::mem;

#[derive(Clone)]
#[repr(C)]
pub struct MemoryMapEntry {
    pub addr: u64,
    pub length: u64,
    pub m_type: u32,
    pub reserved: u32,
}

#[repr(C)]
#[allow(dead_code)]
pub struct MultibootTagMemoryMap {
    s_type: u32,
    size: u32,
    entry_size: u32,
    entry_version: u32,
    entries: MemoryMapEntry,
}

#[derive(Clone)]
pub struct MemoryMapInfo {
    pub address: usize,
    pub num_of_entry: usize,
    pub entry_size: usize,
    cnt: usize,
}

impl MemoryMapInfo {
    pub fn new(map: &MultibootTagMemoryMap) -> Self {
        Self {
            num_of_entry: ((map.size as usize - mem::size_of::<MultibootTagMemoryMap>())
                / map.entry_size as usize),
            address: &map.entries as *const MemoryMapEntry as usize,
            entry_size: map.entry_size as usize,
            cnt: 0,
        }
    }
}

impl Iterator for MemoryMapInfo {
    type Item = &'static MemoryMapEntry;
    fn next(&mut self) -> Option<Self::Item> {
        if self.cnt == self.num_of_entry {
            None
        } else {
            let entry =
                unsafe { &*((self.address + self.cnt * self.entry_size) as *const MemoryMapEntry) };
            self.cnt += 1;
            Some(entry)
        }
    }
}

impl Default for MemoryMapInfo {
    fn default() -> Self {
        Self {
            address: 0,
            num_of_entry: 0,
            entry_size: 0,
            cnt: 0,
        }
    }
}
