//!
//! Peripheral Component Interconnect
//!

pub mod ecam;

use self::ecam::Ecam;

use crate::arch::target_arch::device::pci::{setup_arch_depend_devices, ArchDependPciManager};

use crate::kernel::drivers::acpi::table::mcfg::McfgManager;
use crate::kernel::drivers::device::lpc::LpcManager;
use crate::kernel::drivers::device::nvme::NvmeManager;
use crate::kernel::memory_manager::data_type::{MSize, VAddress};

use alloc::vec::Vec;

pub trait PciDeviceDriver {
    const BASE_CLASS_CODE: u8;
    const SUB_CLASS_CODE: u8;
    fn setup_device(pci_dev: &PciDevice, class_code: ClassCode);
}

enum PciAccessType {
    ArchDepend(ArchDependPciManager),
    Ecam(Ecam),
}

pub struct PciManager {
    access: PciAccessType,
    device_list: Vec<PciDevice>,
}

pub struct PciDevice {
    pub base_address: Option<VAddress>,
    pub address_length: MSize,
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ClassCode {
    pub base: u8,
    pub sub: u8,
    pub programming_interface: u8,
    pub revision: u8,
}

impl PciManager {
    const INVALID_VENDOR_ID: u16 = 0xffff;

    pub fn new_arch_depend(arch_pci_manager: ArchDependPciManager) -> Self {
        Self {
            access: PciAccessType::ArchDepend(arch_pci_manager),
            device_list: Vec::new(),
        }
    }

    pub fn new_ecam(mcfg: McfgManager) -> Self {
        Self {
            access: PciAccessType::Ecam(Ecam::new(mcfg)),
            device_list: Vec::new(),
        }
    }

    pub fn build_device_tree(&mut self) -> Result<(), ()> {
        let (start_bus, end_bus) = match &self.access {
            PciAccessType::ArchDepend(a) => (a.get_start_bus(), a.get_end_bus()),
            PciAccessType::Ecam(e) => (e.get_start_bus(), e.get_end_bus()),
        };
        for bus in start_bus..=end_bus {
            self.build_device_tree_bus(bus)?;
        }
        return Ok(());
    }

    fn build_device_tree_bus(&mut self, bus: u8) -> Result<(), ()> {
        for device in 0..32 {
            self.build_device_tree_device(bus, device)?;
        }
        return Ok(());
    }

    fn build_device_tree_device(&mut self, bus: u8, device: u8) -> Result<(), ()> {
        for function in 0..8 {
            let pci_dev = match &mut self.access {
                PciAccessType::ArchDepend(a) => a.create_pci_device_struct(bus, device, function),
                PciAccessType::Ecam(e) => e.create_pci_device_struct(bus, device, function),
            }?;
            if self.read_vendor_id(&pci_dev)? == Self::INVALID_VENDOR_ID {
                match &mut self.access {
                    PciAccessType::ArchDepend(a) => a.delete_pci_device_struct(pci_dev),
                    PciAccessType::Ecam(e) => e.delete_pci_device_struct(pci_dev),
                }
                if function == 0 {
                    return Ok(());
                } else {
                    continue;
                }
            }
            if function == 0 {
                let header_type = self.read_header_type(&pci_dev)?;
                if (header_type & (1 << 7)) == 0 {
                    self.device_list.push(pci_dev);
                    return Ok(());
                }
            }
            self.device_list.push(pci_dev);
        }

        return Ok(());
    }

    pub fn read_data(&self, pci_dev: &PciDevice, offset: u32, size: u8) -> Result<u32, ()> {
        let aligned_offset = offset & !0b11;
        let data = match &self.access {
            PciAccessType::ArchDepend(a) => a.read_data_pci_dev(pci_dev, aligned_offset),
            PciAccessType::Ecam(e) => e.read_data_pci_dev(pci_dev, aligned_offset),
        }?;

        let byte_offset = (offset & 0b11) as u8;
        assert!(byte_offset + size <= 4);
        return Ok(if size == 4 {
            data
        } else {
            (data >> (byte_offset << 3)) & ((1 << (size << 3)) - 1)
        });
    }

    pub fn write_data(&self, pci_dev: &PciDevice, offset: u32, data: u32) -> Result<(), ()> {
        if (offset & 0b11) != 0 {
            return Err(());
        }
        match &self.access {
            PciAccessType::ArchDepend(a) => a.write_pci_dev(pci_dev, offset, data),
            PciAccessType::Ecam(e) => e.write_data_pci_dev(pci_dev, offset, data),
        }
    }

    pub fn read_data_by_device_number(
        &self,
        bus: u8,
        device: u8,
        function: u8,
        offset: u32,
        size: u8,
    ) -> Result<u32, ()> {
        for e in self.device_list.iter() {
            if e.bus == bus && e.device == device && e.function == function {
                return self.read_data(e, offset, size);
            }
        }
        return Err(());
    }

    pub fn write_data_by_device_number(
        &self,
        bus: u8,
        device: u8,
        function: u8,
        offset: u32,
        data: u32,
    ) -> Result<(), ()> {
        for e in self.device_list.iter() {
            if e.bus == bus && e.device == device && e.function == function {
                return self.write_data(e, offset, data);
            }
        }
        return Err(());
    }

    pub fn read_vendor_id(&self, pci_dev: &PciDevice) -> Result<u16, ()> {
        self.read_data(pci_dev, 0, 2).and_then(|d| Ok(d as u16))
    }

    pub fn read_header_type(&self, pci_dev: &PciDevice) -> Result<u8, ()> {
        self.read_data(pci_dev, 0xc + 2, 1)
            .and_then(|d| Ok(d as u8))
    }

    pub fn read_class_code(&self, pci_dev: &PciDevice) -> Result<ClassCode, ()> {
        let class_and_revision = self.read_data(pci_dev, 0x08, 4)?;
        Ok(ClassCode {
            base: (class_and_revision >> 24) as u8,
            sub: ((class_and_revision >> 16) & 0xff) as u8,
            programming_interface: ((class_and_revision >> 8) & 0xff) as u8,
            revision: (class_and_revision & 0xff) as u8,
        })
    }

    pub fn read_base_address_register(&self, pci_dev: &PciDevice, index: u8) -> Result<u32, ()> {
        if index > 5 {
            return Err(());
        }
        self.read_data(pci_dev, 0x10 + ((index as u32) << 2), 4)
    }

    pub fn setup_devices(&self) {
        for e in &self.device_list {
            let class_code = match self.read_class_code(e) {
                Ok(c) => c,
                Err(e) => {
                    pr_err!("Failed to get the ClassCode: {:?}", e);
                    return;
                }
            };
            /* TODO: Better driver detection */
            if class_code.base == LpcManager::BASE_CLASS_CODE
                && class_code.sub == LpcManager::SUB_CLASS_CODE
            {
                LpcManager::setup_device(e, class_code);
            } else if class_code.base == NvmeManager::BASE_CLASS_CODE
                && class_code.sub == NvmeManager::SUB_CLASS_CODE
            {
                NvmeManager::setup_device(e, class_code);
            } else {
                setup_arch_depend_devices(e, class_code);
            }
        }
    }
}
