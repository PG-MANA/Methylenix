/*
 * Copyright 2017 PG_MANA
 *
 * This software is Licensed under the Apache License Version 2.0
 * See LICENSE.md
 *
 * MultiBoot2実装(Elf解析)
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

//issues:cfgが動かない
//セクションヘッダ(テーブル)
//http://softwaretechnique.jp/OS_Development/Tips/ELF/elf01.html
//#[cfg(not(feature = "elf32"))]//64bit仕様 cfg:http://doc.crates.io/manifest.html#the-features-section
#[repr(C)]
pub struct ElfSection {
    section_name: u32,
    section_type: u32,
    section_flags: u64,
    section_addr: u64,
    section_offset: u64,
    section_size: u64,
    section_link: u32,
    section_info: u32,
    section_addralign: u64,
    section_entry_size: u64,
}

/*#[cfg(feature = "elf32")]
#[repr(C)]
pub struct ElfSection {
    pub section_name: u32,
    pub section_type: u32,
    pub section_flags: u32,
    pub section_addr: u32,
    pub section_offset: u32,
    pub section_size: u32,
    pub section_link: u32,
    pub section_info: u32,
    pub section_addralign: u32,
    pub section_entry_size: u32,
}*/

#[derive(Clone)]
pub struct ElfInfo {
    //MultiBootのElf情報
    pub addr: usize,
    pub num: u32, //firstを含めたelf headerの数
    pub entsize: u32,
    cnt: u32,
}

impl ElfInfo {
    pub fn new(elf: &MultibootTagElfSections) -> ElfInfo {
        ElfInfo {
            addr: unsafe { &elf.sections as *const _ as usize },
            entsize: elf.entsize,
            num: elf.num - 1, //cntが0からカウントするため
            cnt: 0,
        }
    }
    pub fn reset(&mut self) {
        self.cnt = 0;
    }
}

impl Iterator for ElfInfo {
    type Item = &'static ElfSection;
    //                            ↓の'はライフタイムと呼ばれ、返したあとにElfSectionが消えないようにするため。
    fn next(&mut self) -> Option<&'static ElfSection> {
        //これの実装でfor ... inが使える https://rustbyexample.com/trait/iter.html
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
    pub fn size(&self) -> u64 {
        self.section_size as u64
    }
    pub fn flags(&self) -> u64 {
        //列挙型使えるとな...
        self.section_flags as u64
    }
}
