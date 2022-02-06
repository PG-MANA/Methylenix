//!
//! PCI arch-depend
//!

pub mod msi;
pub mod nvme;
pub mod sm_bus;

use crate::arch::target_arch::device::cpu;
use crate::arch::target_arch::device::pci::sm_bus::SmbusManager;

use crate::kernel::drivers::pci::{ClassCode, PciDevice, PciDeviceDriver};
use crate::kernel::memory_manager::data_type::MSize;
use crate::kernel::sync::spin_lock::SpinLockFlag;

pub struct ArchDependPciManager {
    register_lock: SpinLockFlag,
}

impl ArchDependPciManager {
    const CONFIG_ADDRESS: u16 = 0xcf8;
    const CONFIG_DATA: u16 = 0xcfc;

    pub fn new() -> Self {
        Self {
            register_lock: SpinLockFlag::new(),
        }
    }

    pub fn create_pci_device_struct(
        &mut self,
        bus: u8,
        device: u8,
        function: u8,
    ) -> Result<PciDevice, ()> {
        if device >= 32 && function >= 8 {
            return Err(());
        }
        Ok(PciDevice {
            base_address: None,
            address_length: MSize::new(0),
            bus,
            device,
            function,
        })
    }

    pub fn delete_pci_device_struct(&mut self, _pci_dev: PciDevice) {
        return;
    }

    pub fn get_start_bus(&self) -> u8 {
        0
    }

    pub fn get_end_bus(&self) -> u8 {
        0xff
    }

    pub fn read_data_pci_dev(&self, pci_dev: &PciDevice, offset: u32) -> Result<u32, ()> {
        if offset > 0xFF {
            return Err(());
        }
        Ok(self.read_data(pci_dev.bus, pci_dev.device, pci_dev.function, offset as u8))
    }

    pub fn write_pci_dev(&self, pci_dev: &PciDevice, offset: u32, data: u32) -> Result<(), ()> {
        if offset > 0xFF {
            return Err(());
        }
        Ok(self.write_data(
            pci_dev.bus,
            pci_dev.device,
            pci_dev.function,
            offset as u8,
            data,
        ))
    }

    fn read_data(&self, bus: u8, device: u8, function: u8, offset: u8) -> u32 {
        let config_address = Self::calculate_config_address(bus, device, function, offset);

        let _lock = self.register_lock.lock();
        self.write_config_address_register(config_address);
        let data = self.read_config_data_register();
        drop(_lock);
        return data;
    }

    fn write_data(&self, bus: u8, device: u8, function: u8, offset: u8, data: u32) {
        let config_address = Self::calculate_config_address(bus, device, function, offset);
        let _lock = self.register_lock.lock();
        self.write_config_address_register(config_address);
        self.write_config_data_register(data);
        drop(_lock);
        return;
    }

    fn calculate_config_address(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
        (1 << 31/* Enable */)
            | ((bus as u32) << 16)
            | ((device as u32) << 11)
            | ((function as u32) << 8)
            | (offset as u32)
    }

    fn write_config_address_register(&self, data: u32) {
        unsafe { cpu::out_dword(Self::CONFIG_ADDRESS, data) }
    }

    fn read_config_data_register(&self) -> u32 {
        unsafe { cpu::in_dword(Self::CONFIG_DATA) }
    }
    fn write_config_data_register(&self, data: u32) {
        unsafe { cpu::out_dword(Self::CONFIG_DATA, data) }
    }
}

pub fn setup_arch_depend_devices(pci_dev: &PciDevice, class_code: ClassCode) {
    if class_code.base == SmbusManager::BASE_CLASS_CODE
        && class_code.sub == SmbusManager::SUB_CLASS_CODE
    {
        SmbusManager::setup_device(pci_dev, class_code);
    }
}
