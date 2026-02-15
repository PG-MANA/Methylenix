//!
//! BootInformation to pass the kernel
//!

use crate::kernel::file_manager::elf::{Elf64Header, Elf64ProgramHeader};

//use crate::efi::EfiSystemTable;
//use crate::efi::protocol::graphics_output_protocol::EfiGraphicsOutputModeInformation;

pub struct BootInformation {
    pub elf_header_buffer: [u8; size_of::<Elf64Header>()],
    pub elf_program_headers: [Elf64ProgramHeader; 16],
    //pub efi_system_table: EfiSystemTable,
    //pub graphic_info: Option<GraphicInfo>,
    //pub font_address: Option<(usize, usize)>,
    pub ram_map: [RamMapEntry; 32],
    pub memory_info: MemoryInfo,
}

pub struct RamMapEntry {
    pub base: usize,
    pub size: usize,
}

#[allow(dead_code)]
pub struct MemoryInfo {
    pub efi_descriptor_version: u32,
    pub efi_descriptor_size: usize,
    pub efi_memory_map_size: usize,
    pub efi_memory_map_address: usize,
}
