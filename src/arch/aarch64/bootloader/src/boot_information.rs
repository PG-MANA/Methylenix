//!
//! BootInformation to pass the kernel
//!

use crate::efi::protocol::graphics_output_protocol::EfiGraphicsOutputModeInformation;
use crate::efi::EfiSystemTable;
use crate::ELF_64_HEADER_SIZE;

pub struct BootInformation {
    pub elf_header_buffer: [u8; ELF_64_HEADER_SIZE],
    pub elf_program_header_address: usize,
    pub efi_system_table: EfiSystemTable,
    pub graphic_info: Option<GraphicInfo>,
    pub font_address: Option<(usize, usize)>,
    pub memory_info: MemoryInfo,
}

#[allow(dead_code)]
pub struct MemoryInfo {
    pub efi_descriptor_version: u32,
    pub efi_descriptor_size: usize,
    pub efi_memory_map_size: usize,
    pub efi_memory_map_address: usize,
}

#[allow(dead_code)]
pub struct GraphicInfo {
    pub frame_buffer_base: usize,
    pub frame_buffer_size: usize,
    pub info: EfiGraphicsOutputModeInformation,
}
