//!
//! BootInformation Structure from bootloader
//!

use crate::kernel::drivers::efi::protocol::graphics_output_protocol::EfiGraphicsOutputModeInformation;
use crate::kernel::drivers::efi::EfiSystemTable;
use crate::kernel::file_manager::elf::ELF64_HEADER_SIZE;

#[derive(Clone)]
pub struct BootInformation {
    pub elf_header_buffer: [u8; ELF64_HEADER_SIZE],
    pub elf_program_header_address: usize,
    pub efi_system_table: EfiSystemTable,
    pub graphic_info: Option<GraphicInfo>,
    pub font_address: Option<(usize, usize)>,
    pub memory_info: MemoryInfo,
}

#[derive(Clone)]
pub struct MemoryInfo {
    #[allow(dead_code)]
    efi_descriptor_version: u32,
    pub efi_descriptor_size: usize,
    pub efi_memory_map_size: usize,
    pub efi_memory_map_address: usize,
}

#[derive(Clone)]
pub struct GraphicInfo {
    pub frame_buffer_base: usize,
    pub frame_buffer_size: usize,
    pub info: EfiGraphicsOutputModeInformation,
}
