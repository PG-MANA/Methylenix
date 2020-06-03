/*
 * Multiboot Elf Information
 */

#[repr(C, packed)]
pub struct MultibootTagElfSections {
    s_type: u32,
    size: u32,
    num: u32,
    entsize: u32,
    shndx: u32,
    sections: ElfSection,
}

#[repr(C)]
pub struct ElfSection {
    section_name: u32,
    section_type: u32,
    section_flags: usize,
    section_addr: usize,
    section_offset: usize,
    section_size: usize,
    section_link: u32,
    section_info: u32,
    section_addralign: usize,
    section_entry_size: usize,
}

#[derive(Clone)]
pub struct ElfInfo {
    pub address: usize,
    pub size_of_entry: usize,
    pub num_of_entry: usize,
    cnt: usize,
}

impl ElfInfo {
    pub fn new(elf: &MultibootTagElfSections) -> ElfInfo {
        ElfInfo {
            address: unsafe { &elf.sections as *const _ as usize },
            size_of_entry: elf.entsize as usize,
            num_of_entry: elf.num as usize,
            cnt: 0,
        }
    }
}

impl Iterator for ElfInfo {
    type Item = &'static ElfSection;
    fn next(&mut self) -> Option<&'static ElfSection> {
        if self.cnt == self.num_of_entry {
            None
        } else {
            let section =
                unsafe { &*((self.address + self.cnt * self.size_of_entry) as *const ElfSection) };
            self.cnt += 1;
            if section.section_type == 0 {
                self.next()
            } else {
                Some(section)
            }
        }
    }
}

impl Default for ElfInfo {
    fn default() -> ElfInfo {
        ElfInfo {
            address: 0,
            num_of_entry: 0,
            size_of_entry: 0,
            cnt: 0,
        }
    }
}

impl ElfSection {
    pub fn addr(&self) -> usize {
        self.section_addr as usize
    }
    pub fn size(&self) -> usize {
        self.section_size
    }
    pub fn align_size(&self) -> usize {
        self.section_addralign as usize
    }
    pub fn flags(&self) -> usize {
        self.section_flags
    }
    pub fn should_writable(&self) -> bool {
        self.section_flags & 1 != 0
    }
    pub fn should_excusable(&self) -> bool {
        self.section_flags & 4 != 0
    }
    pub fn should_allocate(&self) -> bool {
        self.section_flags & 2 != 0
    }
}
