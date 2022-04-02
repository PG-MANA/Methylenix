//!
//! ELF
//!
//! This file is from kernel/file_manager/elf.rs

use core::marker::PhantomData;

const ELF_MAGIC: [u8; 4] = [0x7f, 0x45, 0x4c, 0x46];
const ELF_CLASS: u8 = 0x02;
const ELF_HEADER_VERSION: u8 = 0x01;

const ELF_SUPPORTED_VERSION: u32 = 1;

pub const ELF_PROGRAM_HEADER_SEGMENT_LOAD: u32 = 0x01;
const ELF_PROGRAM_HEADER_FLAGS_EXECUTABLE: u32 = 0x01;
const ELF_PROGRAM_HEADER_FLAGS_WRITABLE: u32 = 0x02;
const ELF_PROGRAM_HEADER_FLAGS_READABLE: u32 = 0x04;

const ELF_TYPE_EXECUTABLE: u16 = 2;

pub const ELF_MACHINE_AA64: u16 = 183;

#[repr(C)]
pub struct Elf64Header {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

#[repr(C)]
pub struct Elf64ProgramHeader {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

pub struct Elf64ProgramHeaderIter {
    pointer: usize,
    size: u16,
    remaining: u16,
}

pub struct Elf64ProgramHeaderIterMut<'a> {
    pointer: usize,
    size: u16,
    remaining: u16,
    phantom: PhantomData<&'a Elf64ProgramHeader>,
}

impl Elf64Header {
    pub unsafe fn from_ptr(address: &[u8]) -> Result<&Self, ()> {
        let s = &*(address.as_ptr() as *const Self);
        if s.e_ident[0..4] != ELF_MAGIC
            || s.e_ident[4] != ELF_CLASS
            || s.e_ident[6] != ELF_HEADER_VERSION
            || s.e_version != ELF_SUPPORTED_VERSION
        {
            return Err(());
        }
        Ok(s)
    }

    pub const fn is_executable_file(&self) -> bool {
        self.e_type == ELF_TYPE_EXECUTABLE
    }

    pub const fn get_machine_type(&self) -> u16 {
        self.e_machine
    }

    pub const fn get_entry_point(&self) -> u64 {
        self.e_entry
    }

    const fn get_num_of_program_header(&self) -> u16 {
        self.e_phnum
    }

    pub const fn get_program_header_offset(&self) -> u64 {
        self.e_phoff
    }

    pub const fn get_program_header_array_size(&self) -> u64 {
        self.get_num_of_program_header() as u64 * self.get_program_header_entry_size() as u64
    }

    const fn get_program_header_entry_size(&self) -> u16 {
        self.e_phentsize
    }

    pub fn get_program_header_iter(&self, base_address: usize) -> Elf64ProgramHeaderIter {
        Elf64ProgramHeaderIter {
            pointer: base_address,
            size: self.get_program_header_entry_size(),
            remaining: self.get_num_of_program_header(),
        }
    }

    pub fn get_program_header_iter_mut<'a>(
        &self,
        base_address: &'a mut [u8],
    ) -> Elf64ProgramHeaderIterMut<'a> {
        assert!(base_address.len() >= self.get_program_header_array_size() as usize);
        Elf64ProgramHeaderIterMut {
            pointer: base_address.as_ptr() as usize,
            size: self.get_program_header_entry_size(),
            remaining: self.get_num_of_program_header(),
            phantom: PhantomData,
        }
    }
}

impl Iterator for Elf64ProgramHeaderIter {
    type Item = &'static Elf64ProgramHeader;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            None
        } else {
            let r = unsafe { &*(self.pointer as *const Elf64ProgramHeader) };
            self.pointer += self.size as usize;
            self.remaining -= 1;
            Some(r)
        }
    }
}

impl<'a> Iterator for Elf64ProgramHeaderIterMut<'a> {
    type Item = &'a mut Elf64ProgramHeader;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            None
        } else {
            let r = unsafe { &mut *(self.pointer as *mut Elf64ProgramHeader) };
            self.pointer += self.size as usize;
            self.remaining -= 1;
            Some(r)
        }
    }
}

impl Elf64ProgramHeader {
    pub const fn get_segment_type(&self) -> u32 {
        self.p_type
    }

    pub const fn get_file_offset(&self) -> u64 {
        self.p_offset
    }

    pub const fn get_virtual_address(&self) -> u64 {
        self.p_vaddr
    }

    pub const fn get_physical_address(&self) -> u64 {
        self.p_paddr
    }

    pub fn set_physical_address(&mut self, address: u64) {
        self.p_paddr = address;
    }

    pub const fn get_memory_size(&self) -> u64 {
        self.p_memsz
    }

    pub const fn get_file_size(&self) -> u64 {
        self.p_filesz
    }

    pub const fn get_align(&self) -> u64 {
        self.p_align
    }

    pub const fn is_segment_readable(&self) -> bool {
        (self.p_flags & ELF_PROGRAM_HEADER_FLAGS_READABLE) != 0
    }

    pub const fn is_segment_writable(&self) -> bool {
        (self.p_flags & ELF_PROGRAM_HEADER_FLAGS_WRITABLE) != 0
    }

    pub const fn is_segment_executable(&self) -> bool {
        (self.p_flags & ELF_PROGRAM_HEADER_FLAGS_EXECUTABLE) != 0
    }
}
