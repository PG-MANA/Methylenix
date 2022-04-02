//!
//! EFI Simple File System Protocol and EFI File Protocol
//!

use super::super::{EfiStatus, Guid};

pub const EFI_FILE_MODE_READ: u64 = 0x0000000000000001;

pub const EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID: Guid = Guid {
    d1: 0x0964e5b22,
    d2: 0x6459,
    d3: 0x11d2,
    d4: [0x8e, 0x39, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
};

#[repr(C)]
pub struct EfiSimpleFileProtocol {
    revision: u64,
    pub open_volume: extern "efiapi" fn(&Self, &mut *const EfiFileProtocol) -> EfiStatus,
}

#[repr(C)]
pub struct EfiFileProtocol {
    revision: u64,
    pub open: extern "efiapi" fn(&Self, &mut *const Self, *const u16, u64, u64) -> EfiStatus,
    pub close: extern "efiapi" fn(&Self) -> EfiStatus,
    delete: usize,
    pub read: extern "efiapi" fn(&Self, buffer_size: *mut usize, buffer: *mut u8) -> EfiStatus,
    write: usize,
    get_position: usize,
    pub set_position: extern "efiapi" fn(&Self, u64) -> EfiStatus,
    get_info: usize,
    set_info: usize,
    flush: usize,
    open_ex: usize,
    read_ex: usize,
    write_ex: usize,
    flush_ex: usize,
}
