//!
//! Intel ICHx LPC Interface
//!

use crate::kernel::drivers::pci::{ClassCode, PciDevice, PciDeviceDriver};
use crate::kernel::manager_cluster::get_kernel_manager_cluster;

pub struct LpcManager {}

impl PciDeviceDriver for LpcManager {
    const BASE_CLASS_CODE: u8 = 0x06;
    const SUB_CLASS_CODE: u8 = 0x01;

    fn setup_device(pci_dev: &PciDevice, _class_code: ClassCode) -> Result<(), ()> {
        let pci_manager = &get_kernel_manager_cluster().pci_manager;
        let enable_bit = |offset: u32, bit: u32| -> Result<(), ()> {
            let original_data = match pci_manager.read_data(pci_dev, offset, 4) {
                Ok(d) => d,
                Err(e) => {
                    pr_err!("Failed to get the original data: {:?}", e);
                    return Err(());
                }
            };
            if let Err(e) = pci_manager.write_data(pci_dev, offset, original_data | bit) {
                pr_err!("Failed to enable bit: {:?}", e);
                return Err(());
            }
            return Ok(());
        };
        enable_bit(Self::ACPI_CONTROL, Self::ACPI_ENABLE)?;
        enable_bit(Self::GPIO_CONTROL, Self::GPIO_ENABLE)?;
        /* Enable(62h and 66h) Micro controller in LPC_EN */
        enable_bit(Self::LPC_I, Self::MC_LPC_EN << 16)?;
        return Ok(());
    }
}

impl LpcManager {
    const ACPI_CONTROL: u32 = 0x44;
    const GPIO_CONTROL: u32 = 0x4C;
    const LPC_I: u32 = 0x80;

    const ACPI_ENABLE: u32 = 1 << 7;
    const GPIO_ENABLE: u32 = 1 << 4;
    const MC_LPC_EN: u32 = 1 << 11;
}
