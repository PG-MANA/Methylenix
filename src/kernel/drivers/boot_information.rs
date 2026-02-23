//
// BootInformation Structure from bootloader
//
// This comment is not the doc comment because this file is included by the loader.
//

use crate::kernel::drivers::efi::{
    EfiSystemTable, memory_map::EfiMemoryDescriptor,
    protocol::graphics_output_protocol::EfiGraphicsOutputModeInformation,
};
use crate::kernel::file_manager::elf::{Elf64Header, Elf64ProgramHeader};

use core::num::NonZeroUsize;

#[derive(Clone)]
pub struct BootInformation {
    pub elf_header_buffer: [u8; size_of::<Elf64Header>()],
    pub elf_program_headers: [Elf64ProgramHeader; 16],
    pub memory_map: [EfiMemoryDescriptor; 64],
    pub efi_system_table: Option<EfiSystemTable>,
    pub dtb_address: Option<NonZeroUsize>,
    pub graphic_info: Option<GraphicInfo>,
}

#[derive(Clone)]
pub struct GraphicInfo {
    pub frame_buffer_base: usize,
    pub frame_buffer_size: usize,
    pub info: EfiGraphicsOutputModeInformation,
}
