//!
//! Intel ICHx LPC Interface
//!

use crate::kernel::drivers::pci::PciManager;

pub struct LpcManager {}

impl LpcManager {
    pub const BASE_ID: u8 = 0x06;
    pub const SUB_ID: u8 = 0x01;

    const ACPI_CONTROL: u8 = 0x44;
    const GPIO_CONTROL: u8 = 0x4C;
    const LPC_I: u8 = 0x80;

    const ACPI_ENABLE: u32 = 1 << 7;
    const GPIO_ENABLE: u32 = 1 << 4;
    const MC_LPC_EN: u32 = 1 << 11;

    pub fn setup(pci_manager: &PciManager, bus: u8, device: u8, function: u8, _header_type: u8) {
        pci_manager.write_config_address_register(bus, device, function, Self::ACPI_CONTROL);
        pci_manager.write_config_data_register(
            pci_manager.read_config_data_register() | Self::ACPI_ENABLE,
        );

        pci_manager.write_config_address_register(bus, device, function, Self::GPIO_CONTROL);
        pci_manager.write_config_data_register(
            pci_manager.read_config_data_register() | Self::GPIO_ENABLE,
        );

        pci_manager.write_config_address_register(bus, device, function, Self::LPC_I);
        pci_manager.write_config_data_register(
            pci_manager.read_config_data_register() | (Self::MC_LPC_EN << 16),
        ); /* Enable(62h and 66h) Micro controller in LPC_EN */
    }
}
