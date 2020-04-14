//EFI Tableの実装
//https://uefi.org/sites/default/files/resources/UEFI_Spec_2_7.pdf

//use
use super::text::output::EfiOutputProtocol;

//type
pub type EfiHandle = usize;
//本当はポインタ
pub type EfiStatus = usize;

#[repr(C)]
#[derive(Clone)]
pub struct EfiTableHeader {
    signature: u64,
    revision: u32,
    header_size: u32,
    crc32: u32,
    reserved: u32,
}

#[repr(C)]
pub struct EfiBootServices {
    raise_tpl: usize,
    restore_tpl: usize,
    allocate_pages: usize,
    free_pages: usize,
    get_memory_map: usize,
    allocate_pool: usize,
    free_pool: usize,
    create_event: usize,
    set_timer: usize,
    exit: usize,
    unload_image: usize,
    exit_boot_services: usize,
    get_next_monotonic_count: usize,
    stall: usize,
    set_watchdog_timer: usize,
    connect_controller: usize,
    disconnect_controller: usize,
    open_protocol: usize,
    close_protocol: usize,
    open_protocol_information: usize,
    protocols_per_handle: usize,
    locate_handle_buffer: usize,
    locate_protocol: usize,
    install_multiple_protocol_interfaces: usize,
    uninstall_multiple_protocol_interfaces: usize,
    calculate_crc32: usize,
    copy_mem: usize,
    set_mem: usize,
    reate_event_ex: usize,
}

#[repr(C)]
#[derive(Clone)]
pub struct EfiSystemTable {
    pub efi_table_header: EfiTableHeader,
    pub firmware_vender: usize,
    pub firmware_version: u32,
    pub console_input_handler: EfiHandle,
    pub console_input_protocol: usize,
    //*const EfiInputProtocol,
    pub console_output_handler: EfiHandle,
    pub console_output_protocol: *const EfiOutputProtocol,
    pub standard_error_handler: EfiHandle,
    pub standard_error_protocol: *const EfiOutputProtocol,
    pub efi_runtime_services: usize,
    //未実装
    pub efi_boot_services: *const EfiBootServices,
    //未実装
    pub num_table_entries: usize,
    pub configuration_table: usize, //未実装
}

pub struct EfiTableManager {
    address: *const EfiSystemTable,
}

impl EfiTableManager {
    const EFI_SYSTEM_TABLE_SIGNATURE: u64 = 0x5453595320494249;
    pub fn new(table_adress: usize) -> EfiTableManager {
        if table_adress == 0 {
            panic!("Invalid EFI Table Address");
        }
        let system_table = unsafe { &*(table_adress as *const EfiSystemTable) };
        if system_table.efi_table_header.signature != EfiTableManager::EFI_SYSTEM_TABLE_SIGNATURE {
            panic!("Invalid EFI Table");
        }
        EfiTableManager {
            address: system_table,
        }
    }

    pub const fn new_static() -> EfiTableManager {
        EfiTableManager {
            address: 0 as *const EfiSystemTable,
        }
    }

    pub fn get_efi_systemtable(&self) -> &'static EfiSystemTable {
        //unsafe { (*self.address).clone() } //Getter作るのめんどくさかった
        unsafe { &*self.address }
    }
}
