//!
//! PCIe Enhanced Configuration Access Mechanism
//!

use crate::io_remap;
use crate::kernel::drivers::acpi::table::mcfg::McfgManager;
use crate::kernel::drivers::pci::PciDevice;
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{
    Address, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress, VAddress,
};

pub struct Ecam {
    ecam_base_address: PAddress,
    start_bus: u8,
    end_bus: u8,
}

impl Ecam {
    pub fn new(mcfg: McfgManager) -> Self {
        /* Currently supports one segment only */
        let info = mcfg
            .get_base_address_info(0)
            .expect("Failed to get Base Address information");

        pr_info!(
            "PCI Bus Base Address: {:#X}, Bus: {} ~ {}",
            info.base_address,
            info.start_bus,
            info.end_bus
        );

        Self {
            ecam_base_address: PAddress::new(info.base_address as usize),
            start_bus: info.start_bus,
            end_bus: info.end_bus,
        }
    }

    fn get_mmio_base_address(&self, bus: u8, device: u8, function: u8) -> PAddress {
        self.ecam_base_address
            + MSize::new(
                ((bus as usize) << 20) | ((device as usize) << 15) | ((function as usize) << 12),
            )
    }

    pub fn get_start_bus(&self) -> u8 {
        self.start_bus
    }

    pub fn get_end_bus(&self) -> u8 {
        self.end_bus
    }

    pub fn create_pci_device_struct(
        &mut self,
        bus: u8,
        device: u8,
        function: u8,
    ) -> Result<PciDevice, ()> {
        let map_size = MSize::new(0x1000);
        let map_result = io_remap!(
            self.get_mmio_base_address(bus, device, function),
            map_size,
            MemoryPermissionFlags::data(),
            MemoryOptionFlags::DO_NOT_FREE_PHYSICAL_ADDRESS
        );
        if let Err(e) = map_result {
            pr_err!("Failed to map memory of PCI: {:?}", e);
            return Err(());
        }
        Ok(PciDevice {
            base_address: Some(map_result.unwrap()),
            address_length: map_size,
            bus,
            device,
            function,
        })
    }

    pub fn delete_pci_device_struct(&mut self, pci_dev: PciDevice) {
        if let Some(address) = pci_dev.base_address {
            if let Err(e) = get_kernel_manager_cluster()
                .kernel_memory_manager
                .free(address)
            {
                pr_err!("Failed to free memory mapping: {:?}", e);
            }
        }
        return;
    }

    pub fn read_data(&self, function_base_address: VAddress, offset: usize, size: u8) -> u32 {
        let aligned_offset = MSize::new(offset & !0b11);
        let data = unsafe {
            core::ptr::read_volatile(
                (function_base_address + aligned_offset).to_usize() as *const u32
            )
        };
        let byte_offset = (offset & 0b11) as u8;
        assert!(byte_offset + size <= 4);
        return if size == 4 {
            data
        } else {
            (data >> (byte_offset << 3)) & ((1 << (size << 3)) - 1)
        };
    }

    pub fn read_data_pci_dev(&self, pci_dev: &PciDevice, offset: u32) -> Result<u32, ()> {
        if let Some(base_address) = pci_dev.base_address {
            let offset = MSize::new(offset as usize);
            if offset >= pci_dev.address_length {
                return Err(());
            }
            Ok(unsafe {
                core::ptr::read_volatile((base_address + offset).to_usize() as *const u32)
            })
        } else {
            Err(())
        }
    }

    pub fn write_data_pci_dev(
        &self,
        pci_dev: &PciDevice,
        offset: u32,
        data: u32,
    ) -> Result<(), ()> {
        if let Some(base_address) = pci_dev.base_address {
            let offset = MSize::new(offset as usize);
            if offset >= pci_dev.address_length {
                return Err(());
            }
            unsafe {
                core::ptr::write_volatile((base_address + offset).to_usize() as *mut u32, data)
            };
            Ok(())
        } else {
            Err(())
        }
    }
}
