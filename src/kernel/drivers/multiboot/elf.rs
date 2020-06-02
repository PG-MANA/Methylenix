/*
 * Multiboot Elf Information
 */

#[repr(C, packed)] //sectionsの前に余計なパッディングを入れないため
pub struct MultibootTagElfSections {
    s_type: u32,
    size: u32,
    num: u32,
    entsize: u32,
    shndx: u32,
    sections: ElfSection,
}

//セクションヘッダ(テーブル)
//雑い設計
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
    //MultiBootのElf情報
    pub addr: usize,
    pub num: u32,
    //firstを含めたelf headerの数
    pub entsize: u32,
    cnt: u32,
}

impl ElfInfo {
    pub fn new(elf: &MultibootTagElfSections) -> ElfInfo {
        ElfInfo {
            addr: unsafe { &elf.sections as *const _ as usize },
            entsize: elf.entsize,
            num: elf.num,
            cnt: 0,
        }
    }
    pub fn reset(&mut self) {
        self.cnt = 0;
    }
}

impl Iterator for ElfInfo {
    type Item = &'static ElfSection;
    fn next(&mut self) -> Option<&'static ElfSection> {
        if self.cnt == self.num {
            return None;
        }
        let section =
            unsafe { &*((self.addr + (self.cnt * self.entsize) as usize) as *const ElfSection) };
        self.cnt += 1;
        if section.section_type == 0 {
            //できればEnum使いたい
            return self.next(); //できればelseは使いたくない(C言語での習慣から)
        }
        return Some(section);
    }
}

impl Default for ElfInfo {
    fn default() -> ElfInfo {
        ElfInfo {
            addr: 0,
            num: 0,
            entsize: 0,
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
        //列挙型使えるとな...
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
