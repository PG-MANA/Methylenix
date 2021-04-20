//!
//! PCI arch-depend
//!

mod lpc;
mod sm_bus;

use self::lpc::LpcManager;
use self::sm_bus::SmbusManager;

use crate::arch::target_arch::device::cpu;

use crate::kernel::drivers::pci::{ClassCode, PciManager};

const CONFIG_ADDRESS: u16 = 0xcf8;
const CONFIG_DATA: u16 = 0xcfc;

pub fn write_config_address_register(data: u32) {
    unsafe { cpu::out_dword(CONFIG_ADDRESS, data) }
}

pub fn write_config_data_register(data: u32) {
    unsafe { cpu::out_dword(CONFIG_DATA, data) }
}

pub fn read_config_data_register() -> u32 {
    unsafe { cpu::in_dword(CONFIG_DATA) }
}

pub fn setup_arch_depend_pci_device(
    pci_manager: &PciManager,
    bus: u8,
    device: u8,
    function: u8,
    header_type: u8,
    class_code: ClassCode,
) -> bool {
    if class_code.base == SmbusManager::BASE_ID && class_code.sub == SmbusManager::SUB_ID {
        pr_info!("Detect: SMBus");
        SmbusManager::setup(pci_manager, bus, device, function, header_type);
    } else if class_code.base == LpcManager::BASE_ID && class_code.sub == LpcManager::SUB_ID {
        pr_info!("Detect: LPC");
        LpcManager::setup(pci_manager, bus, device, function, header_type);
    } else {
        return false;
    }
    return true;
}
