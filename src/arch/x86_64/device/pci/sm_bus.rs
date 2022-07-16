//!
//! System Management Bus for Intel Chipset
//!

use crate::arch::target_arch::device::cpu::{in_byte, out_byte};

use crate::kernel::drivers::acpi::aml::ResourceData;
use crate::kernel::drivers::pci::{ClassCode, PciDevice, PciDeviceDriver};
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};

pub struct SmbusManager {}

impl PciDeviceDriver for SmbusManager {
    const BASE_CLASS_CODE: u8 = 0x0C;
    const SUB_CLASS_CODE: u8 = 0x05;

    fn setup_device(pci_dev: &PciDevice, _class_code: ClassCode) -> Result<(), ()> {
        let pci_manager = &get_kernel_manager_cluster().pci_manager;
        macro_rules! read_pci {
            ($offset:expr, $size:expr) => {
                match pci_manager.read_data(pci_dev, $offset, $size) {
                    Ok(d) => d,
                    Err(e) => {
                        pr_err!("Failed to read PCI configuration space: {:?},", e);
                        return Err(());
                    }
                }
            };
        }
        macro_rules! write_pci {
            ($offset:expr, $data:expr) => {
                if let Err(e) = pci_manager.write_data(pci_dev, $offset, $data) {
                    pr_err!("Failed to read PCI configuration space: {:?},", e);
                    return Err(());
                }
            };
        }

        let status = read_pci!(Self::PCI_CMD, 4) >> 16;
        if (status & Self::INTERRUPT_STATUS) != 0 {
            pr_debug!("SMBus interrupt is pending.");
        }
        let smbus_base_address = read_pci!(Self::SMB_BASE, 4);
        if (smbus_base_address & 0b1) == 0 {
            pr_err!("Invalid base address.");
            return Err(());
        }
        let smbus_base_address = ((smbus_base_address & !0b1) & 0xFFFF) as u16;
        pr_debug!("SMBus Base Address: {:#X}", smbus_base_address);

        let interrupt_pin = read_pci!(Self::INT_PIN, 1) as u8;
        if interrupt_pin == 0 || interrupt_pin > 5 {
            pr_err!("SMBus interrupt is disabled.");
            return Err(());
        }
        let int_pin = interrupt_pin - 1;
        pr_debug!("SMBus Interrupt Pin: INT{}#", (int_pin + b'A') as char);
        let resource_data = get_kernel_manager_cluster()
            .acpi_manager
            .lock()
            .unwrap()
            .search_interrupt_information_with_evaluation_aml(pci_dev.bus, pci_dev.device, int_pin);
        if resource_data.is_none() {
            pr_err!("Cannot detect irq.");
            return Err(());
        }
        let irq = match resource_data.unwrap() {
            ResourceData::Irq(i) => i,
            ResourceData::Interrupt(i) => i as u8, /* OK...? */
        };
        pr_debug!("SMBus IRQ: {}", irq);
        if let Err(e) = get_cpu_manager_cluster()
            .interrupt_manager
            .set_device_interrupt_function(smbus_handler, Some(irq), None, 0, false)
        {
            pr_err!("Failed to setup interrupt: {:?}", e);
            return Err(());
        }

        /* Clear Interrupt Disable and enable I/O Space */
        write_pci!(
            Self::PCI_CMD,
            (read_pci!(Self::PCI_CMD, 4) | Self::IO_SPACE_ENABLE) & !(Self::INTERRUPT_DISABLE)
        );

        /* Set SMBus Host Enable Bit */
        write_pci!(
            Self::HOST_CONFIG,
            read_pci!(Self::HOST_CONFIG, 4) | Self::SMBUS_HOST_ENABLE
        );

        unsafe {
            /* Set Interrupt Enable Bit in HST_CNT */
            out_byte(
                smbus_base_address + Self::SMBUS_HOST_CONTROL,
                in_byte(smbus_base_address + Self::SMBUS_HOST_CONTROL)
                    | Self::SMBUS_INTERRUPT_ENABLE,
            );
            /* Set Host Notify Enable Bit in Slave Command Register */
            out_byte(
                smbus_base_address + Self::SMBUS_SLV_CMD,
                in_byte(smbus_base_address + Self::SMBUS_SLV_CMD)
                    | Self::SMBUS_HOST_NOTIFY_INTERRUPT_ENABLE,
            );

            /* Clear Host Notify Status */
            out_byte(
                smbus_base_address + Self::SMBUS_SLV_STATUS,
                Self::SMBUS_HOST_NOTIFY_STATUS,
            );

            /* Clear Interrupt Bit  */
            out_byte(
                smbus_base_address + Self::SMBUS_HOST_STATUS,
                Self::SMBUS_INTERRUPT,
            );
        }
        return Ok(());
    }
}

impl SmbusManager {
    const PCI_CMD: u32 = 0x04;
    const SMB_BASE: u32 = 0x20;
    //const INT_LINE: u32 = 0x3C;
    const INT_PIN: u32 = 0x3D;
    const HOST_CONFIG: u32 = 0x40;

    const IO_SPACE_ENABLE: u32 = 1;
    const INTERRUPT_DISABLE: u32 = 1 << 10;
    const INTERRUPT_STATUS: u32 = 1 << 3;
    const SMBUS_HOST_ENABLE: u32 = 1;

    const SMBUS_HOST_STATUS: u16 = 0;
    const SMBUS_HOST_CONTROL: u16 = 0x02;
    const SMBUS_SLV_STATUS: u16 = 0x10;
    const SMBUS_SLV_CMD: u16 = 0x11;

    const SMBUS_INTERRUPT_ENABLE: u8 = 1;
    const SMBUS_INTERRUPT: u8 = 1 << 1;
    const SMBUS_HOST_NOTIFY_STATUS: u8 = 1;
    const SMBUS_HOST_NOTIFY_INTERRUPT_ENABLE: u8 = 1;
}

fn smbus_handler(_: usize) -> bool {
    pr_info!("Interrupted from SMBus.(Currently, do nothing.)");
    return true;
}
