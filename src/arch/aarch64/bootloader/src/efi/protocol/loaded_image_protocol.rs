//!
//! EFI Loaded Image Protocol
//!

use crate::efi::{memory_map::EfiMemoryType, EfiHandle, EfiSystemTable, Guid};

#[repr(C)]
pub struct EfiLoadedImageProtocol {
    pub revision: u32,
    pub parent_handle: EfiHandle,
    pub system_table: *const EfiSystemTable,
    pub device_handle: EfiHandle,
    pub file_path: usize,
    pub reserved: usize,
    pub load_option_size: u32,
    pub load_options: usize,
    pub image_base: usize,
    pub image_code_type: EfiMemoryType,
    pub image_data_type: EfiMemoryType,
    pub unload: usize,
}

pub const EFI_LOADED_IMAGE_PROTOCOL_GUID: Guid = Guid {
    d1: 0x5B1B31A1,
    d2: 0x9562,
    d3: 0x11d2,
    d4: [0x8E, 0x3F, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
};

pub const EFI_OPEN_PROTOCOL_BY_HANDLE_PROTOCOL: u32 = 0x00000001;
