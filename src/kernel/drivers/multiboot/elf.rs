//!
//! Multiboot ELF Information
//!

use crate::kernel::file_manager::elf::Elf64SectionHeader;

#[repr(C, packed)]
pub struct MultibootTagElfSections {
    s_type: u32,
    size: u32,
    num: u32,
    entsize: u32,
    shndx: u32,
}

#[derive(Clone, Default)]
pub struct ElfInfo {
    pub address: usize,
    pub size_of_entry: usize,
    pub num_of_entry: usize,
    cnt: usize,
}

impl ElfInfo {
    pub fn new(elf: &MultibootTagElfSections) -> Self {
        Self {
            address: (elf as *const _ as usize) + size_of::<MultibootTagElfSections>(),
            size_of_entry: elf.entsize as usize,
            num_of_entry: elf.num as usize,
            cnt: 0,
        }
    }
}

impl Iterator for ElfInfo {
    type Item = &'static Elf64SectionHeader;
    fn next(&mut self) -> Option<Self::Item> {
        if self.cnt == self.num_of_entry {
            None
        } else {
            let section = unsafe {
                &*((self.address + self.cnt * self.size_of_entry) as *const Elf64SectionHeader)
            };
            self.cnt += 1;
            if section.get_section_type() == 0 {
                self.next()
            } else {
                Some(section)
            }
        }
    }
}
