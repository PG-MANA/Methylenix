//!
//! Multiboot Memory Map Information
//!

use crate::kernel::drivers::efi::memory_map::EfiMemoryDescriptor;

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
}

#[derive(Clone, Default)]
pub struct MemoryMapInfo {
    pub address: usize,
    pub num_of_entries: usize,
    pub entry_size: usize,
    count: usize,
}

#[repr(C)]
pub struct MultibootTagEfiMemoryMap {
    s_type: u32,
    size: u32,
    descriptor_size: u32,
    descriptor_version: u32,
}

#[derive(Clone, Default)]
pub struct EfiMemoryMapInfo {
    pub address: usize,
    pub num_of_entries: usize,
    pub entry_size: usize,
    count: usize,
}

impl MemoryMapInfo {
    pub fn new(map: &MultibootTagMemoryMap) -> Self {
        Self {
            num_of_entries: (map.size as usize - mem::size_of::<MultibootTagMemoryMap>())
                / map.entry_size as usize,
            address: map as *const MultibootTagMemoryMap as usize
                + mem::size_of::<MultibootTagMemoryMap>(),
            entry_size: map.entry_size as usize,
            count: 0,
        }
    }
}

impl Iterator for MemoryMapInfo {
    type Item = &'static MemoryMapEntry;
    fn next(&mut self) -> Option<Self::Item> {
        if self.count == self.num_of_entries {
            None
        } else {
            let entry = unsafe {
                &*((self.address + self.count * self.entry_size) as *const MemoryMapEntry)
            };
            self.count += 1;
            Some(entry)
        }
    }
}

impl EfiMemoryMapInfo {
    pub fn new(map: &MultibootTagEfiMemoryMap) -> Self {
        Self {
            num_of_entries: (map.size as usize - mem::size_of::<MultibootTagEfiMemoryMap>())
                / map.descriptor_size as usize,
            address: map as *const MultibootTagEfiMemoryMap as usize
                + mem::size_of::<MultibootTagMemoryMap>(),
            entry_size: map.descriptor_size as usize,
            count: 0,
        }
    }
}

impl Iterator for EfiMemoryMapInfo {
    type Item = &'static EfiMemoryDescriptor;
    fn next(&mut self) -> Option<Self::Item> {
        if self.count == self.num_of_entries {
            None
        } else {
            let entry = unsafe {
                &*((self.address + self.count * self.entry_size) as *const EfiMemoryDescriptor)
            };
            self.count += 1;
            Some(entry)
        }
    }
}
