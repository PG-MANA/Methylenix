use crate::kernel::drivers::pci::{ClassCode, PciDevice, PciDeviceDriver};
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{Address, MSize, MemoryPermissionFlags, PAddress};

use crate::io_remap;

pub struct NvmeManager {}

impl PciDeviceDriver for NvmeManager {
    const BASE_CLASS_CODE: u8 = 0x01;
    const SUB_CLASS_CODE: u8 = 0x08;

    fn setup_device(pci_dev: &PciDevice, class_code: ClassCode) {
        if class_code.programming_interface != 2 && class_code.programming_interface != 3 {
            pr_err!(
                "Unsupported programming interface: {:#X}",
                class_code.programming_interface
            );
            return;
        }
        macro_rules! read_pci {
            ($offset:expr, $size:expr) => {
                match get_kernel_manager_cluster()
                    .pci_manager
                    .read_data(pci_dev, $offset, $size)
                {
                    Ok(d) => d,
                    Err(e) => {
                        pr_err!("Failed to read PCI configuration space: {:?},", e);
                        return;
                    }
                }
            };
        }

        let base_address_0 = read_pci!(0x10, 4);
        if base_address_0 & 0x01 != 0 {
            pr_err!("Expected MMIO");
            return;
        }
        let is_64bit_bar_address = ((base_address_0 >> 1) & 0b11) == 0b10;
        let base_address = (base_address_0 & !0b1111) as usize
            | if is_64bit_bar_address {
                (read_pci!(0x14, 4) as usize) << 32
            } else {
                0
            };
        pr_info!(
            "NVMe BaseAddress: {:#X}(64bit: {})",
            base_address,
            is_64bit_bar_address
        );
        let controller_property_base_address = io_remap!(
            PAddress::new(base_address),
            MSize::new(0x1000),
            MemoryPermissionFlags::data()
        );
        if let Err(e) = controller_property_base_address {
            pr_err!("Failed to map NVMe Controller Properties: {:?}", e);
            return;
        }
        let controller_property_base_address = controller_property_base_address.unwrap();
        let version = unsafe {
            core::ptr::read_volatile(
                (controller_property_base_address.to_usize() + 0x08) as *const u32,
            )
        };
        pr_info!("Controller Property's version: {:#X}", version);

        return;
    }
}
