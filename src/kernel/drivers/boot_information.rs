//
// BootInformation Structure from bootloader
//
// This comment is not the doc comment because this file is included by the loader.
//

use crate::kernel::drivers::efi::{
    EfiSystemTable,
    memory_map::{EfiMemoryAttribute, EfiMemoryDescriptor, EfiMemoryType},
    protocol::graphics_output_protocol::EfiGraphicsOutputModeInformation,
};
use crate::kernel::file_manager::elf::{Elf64Header, Elf64ProgramHeader};
use crate::kernel::memory_manager::data_type::{MSize, PAddress, VAddress};

use core::num::NonZeroUsize;

pub type BootInformationMemoryMap = [EfiMemoryDescriptor; 32];

#[derive(Clone)]
pub struct BootInformation {
    pub elf_header_buffer: [u8; size_of::<Elf64Header>()],
    pub elf_program_headers: [Elf64ProgramHeader; 12],
    pub memory_map: BootInformationMemoryMap,
    pub additional_memory_map: Option<AdditonalMemoryMap>,
    pub efi_system_table: Option<EfiSystemTable>,
    pub dtb_address: Option<NonZeroUsize>,
    pub graphic_info: Option<GraphicInfo>,
    pub font_info: Option<FontInfo>,
}

impl Default for BootInformation {
    fn default() -> Self {
        use core::array;
        Self {
            elf_header_buffer: [0; size_of::<Elf64Header>()],
            elf_program_headers: array::repeat(Elf64ProgramHeader::default()),
            memory_map: array::repeat(EfiMemoryDescriptor {
                memory_type: EfiMemoryType::MaxMemoryType,
                physical_start: 0,
                virtual_start: 0,
                number_of_pages: 0,
                attribute: EfiMemoryAttribute::EfiMemoryUc,
            }),
            additional_memory_map: None,
            efi_system_table: None,
            dtb_address: None,
            graphic_info: None,
            font_info: None,
        }
    }
}

#[derive(Clone)]
pub struct AdditonalMemoryMap {
    pub address: VAddress,
    pub allocated_size: MSize,
    pub actual_size: NonZeroUsize,
}

#[derive(Clone)]
pub struct GraphicInfo {
    pub frame_buffer_address: PAddress,
    pub frame_buffer_size: NonZeroUsize,
    pub info: EfiGraphicsOutputModeInformation,
}

/// Contains the loaded font information
#[derive(Clone)]
pub struct FontInfo {
    pub font_address: VAddress,
    pub font_size: NonZeroUsize,
}
