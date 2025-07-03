//!
//! EFI Functions
//!

pub mod protocol {
    pub mod file_protocol;
    pub mod graphics_output_protocol;
    pub mod loaded_image_protocol;
    pub mod simple_text_output_protocol;
}
pub mod memory_map;

use self::memory_map::{EfiAllocateType, EfiMemoryType};
use self::protocol::simple_text_output_protocol::EfiSimpleTextOutputProtocol;

use crate::kernel::collections::guid::Guid;

pub type EfiStatus = usize;
pub const EFI_SUCCESS: EfiStatus = 0;

pub const EFI_PAGE_SIZE: usize = 0x1000;

pub type EfiHandle = usize;

#[derive(Clone)]
#[repr(C)]
pub struct EfiTableHeader {
    signature: u64,
    revision: u32,
    header_size: u32,
    crc32: u32,
    reserved: u32,
}

#[repr(C)]
pub struct EfiBootServices {
    efi_table_header: EfiTableHeader,
    raise_tpl: usize,
    restore_tpl: usize,
    pub allocate_pages:
        extern "efiapi" fn(EfiAllocateType, EfiMemoryType, usize, &mut usize) -> EfiStatus,
    free_pages: usize,
    pub get_memory_map:
        extern "efiapi" fn(&mut usize, usize, &mut usize, &mut usize, &mut u32) -> EfiStatus,
    allocate_pool: usize,
    free_pool: usize,
    create_event: usize,
    set_timer: usize,
    wait_for_event: usize,
    signal_event: usize,
    close_event: usize,
    check_event: usize,
    install_protocol_interface: usize,
    reinstall_protocol_interface: usize,
    uninstall_protocol_interface: usize,
    handle_protocol: usize,
    reserved: usize,
    register_protocol_notify: usize,
    locate_handle: usize,
    locate_device_path: usize,
    install_configuration_table: usize,
    load_image: usize,
    start_image: usize,
    exit: usize,
    unload_image: usize,
    pub exit_boot_services: extern "efiapi" fn(EfiHandle, usize) -> EfiStatus,
    get_next_monotonic_count: usize,
    stall: usize,
    set_watchdog_timer: usize,
    connect_controller: usize,
    disconnect_controller: usize,
    pub open_protocol:
        extern "efiapi" fn(EfiHandle, &Guid, usize, EfiHandle, EfiHandle, u32) -> EfiStatus,
    close_protocol: usize,
    open_protocol_information: usize,
    protocols_per_handle: usize,
    locate_handle_buffer: usize,
    pub locate_protocol: extern "efiapi" fn(&Guid, usize, usize) -> EfiStatus,
    install_multiple_protocol_interfaces: usize,
    uninstall_multiple_protocol_interfaces: usize,
    calculate_crc32: usize,
    copy_mem: usize,
    set_mem: usize,
    create_event_ex: usize,
}

#[derive(Clone)]
#[repr(C)]
pub struct EfiSystemTable {
    efi_table_header: EfiTableHeader,
    firmware_vendor: usize,
    firmware_version: u32,
    console_input_handler: EfiHandle,
    console_input_protocol: usize,
    console_output_handler: EfiHandle,
    console_output_protocol: *const EfiSimpleTextOutputProtocol,
    standard_error_handler: EfiHandle,
    standard_error_protocol: *const EfiSimpleTextOutputProtocol,
    efi_runtime_services: usize,
    efi_boot_services: *const EfiBootServices,
    num_table_entries: usize,
    configuration_table: usize,
}

#[repr(C)]
pub struct EfiConfigurationTable {
    pub vendor_guid: Guid,
    pub vendor_table: usize,
}

pub const EFI_ACPI_2_0_TABLE_GUID: Guid = Guid {
    d1: 0x8868e871,
    d2: 0xe4f1,
    d3: 0x11d3,
    d4: [0xbc, 0x22, 0x00, 0x80, 0xc7, 0x3c, 0x88, 0x81],
};

pub const EFI_DTB_TABLE_GUID: Guid = Guid {
    d1: 0xb1b621d5,
    d2: 0xf19c,
    d3: 0x41a5,
    d4: [0x83, 0x0b, 0xd9, 0x15, 0x2c, 0x69, 0xaa, 0xe0],
};

impl EfiSystemTable {
    const EFI_SYSTEM_TABLE_SIGNATURE: u64 = 0x5453595320494249;
    pub fn verify(&self) -> bool {
        if self.efi_table_header.signature != Self::EFI_SYSTEM_TABLE_SIGNATURE {
            return false;
        }
        true
    }

    pub const fn get_console_output_protocol(&self) -> *const EfiSimpleTextOutputProtocol {
        self.console_output_protocol
    }

    pub const fn get_boot_services(&self) -> *const EfiBootServices {
        self.efi_boot_services
    }

    pub const fn get_configuration_table(&self) -> usize {
        self.configuration_table
    }

    pub fn set_configuration_table(&mut self, address: usize) {
        self.configuration_table = address;
    }

    pub const fn get_number_of_configuration_tables(&self) -> usize {
        self.num_table_entries
    }

    pub unsafe fn get_configuration_table_slice(&self) -> &[EfiConfigurationTable] {
        unsafe {
            core::slice::from_raw_parts(
                self.get_configuration_table() as *const EfiConfigurationTable,
                self.get_number_of_configuration_tables(),
            )
        }
    }
}
