//!
//! PCI arch-depend
//!

use crate::arch::target_arch::device::cpu;

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
