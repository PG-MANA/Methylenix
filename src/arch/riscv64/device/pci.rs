//!
//! PCI arch-depend
//!

use crate::kernel::drivers::pci::{ClassCode, PciDevice};

pub struct ArchDependPciManager {}

impl Default for ArchDependPciManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ArchDependPciManager {
    pub fn new() -> Self {
        Self {}
    }

    pub fn create_pci_device_struct(
        &mut self,
        _bus: u8,
        _device: u8,
        _function: u8,
    ) -> Result<PciDevice, ()> {
        Err(())
    }

    pub fn delete_pci_device_struct(&mut self, _pci_dev: PciDevice) {}

    pub fn get_start_bus(&self) -> u8 {
        0
    }

    pub fn get_end_bus(&self) -> u8 {
        0xff
    }

    pub fn read_data_pci_dev(&self, _pci_dev: &PciDevice, _offset: u32) -> Result<u32, ()> {
        Err(())
    }

    pub fn write_pci_dev(&self, _pci_dev: &PciDevice, _offset: u32, _data: u32) -> Result<(), ()> {
        Err(())
    }
}

pub fn setup_arch_depend_devices(_pci_dev: &PciDevice, _class_code: ClassCode) {}
