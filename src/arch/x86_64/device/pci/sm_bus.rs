//!
//! System Management Bus for Intel Chipset
//!

use crate::arch::target_arch::device::cpu::{in_byte, out_byte};
use crate::arch::target_arch::interrupt::{InterruptManager, IstIndex};

use crate::kernel::drivers::pci::PciManager;
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};

pub struct SmbusManager {}

impl SmbusManager {
    pub const BASE_ID: u8 = 0x0C;
    pub const SUB_ID: u8 = 0x05;

    const PCI_CMD: u8 = 0x04;
    const SMB_BASE: u8 = 0x20;
    const INT_LINE: u8 = 0x3C;
    const HOST_CONFIG: u8 = 0x40;

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

    pub fn setup(pci_manager: &PciManager, bus: u8, device: u8, function: u8, _header_type: u8) {
        pci_manager.write_config_address_register(bus, device, function, Self::PCI_CMD);
        let status = pci_manager.read_config_data_register() >> 16;
        if (status & Self::INTERRUPT_STATUS) != 0 {
            pr_info!("SMBus interrupt is pending.");
        }
        pci_manager.write_config_address_register(bus, device, function, Self::SMB_BASE);
        let smbus_base_address = ((pci_manager.read_config_data_register() & !0b1) & 0xFFFF) as u16;
        pr_info!("Base address of SMBus: {:#X}", smbus_base_address);

        pci_manager.write_config_address_register(bus, device, function, Self::INT_LINE);
        let interrupt_pin = ((pci_manager.read_config_data_register() >> 8) & 0xFF) as u8;
        if interrupt_pin == 0 || interrupt_pin > 5 {
            pr_err!("SMBus interrupt is disabled.");
            return;
        }
        let int_pin = interrupt_pin - 1;
        pr_info!("Interrupt Pin: INT{}#", (int_pin + b'A') as char);
        let irq = get_kernel_manager_cluster()
            .acpi_manager
            .lock()
            .unwrap()
            .search_intr_number_with_evaluation_aml(bus, device, int_pin);
        if irq.is_none() {
            pr_err!("Cannot detect irq.");
            return;
        }
        pr_info!("IRQ: {}", irq.unwrap());

        make_device_interrupt_handler!(handler, smbus_handler);
        if !get_cpu_manager_cluster()
            .interrupt_manager
            .set_device_interrupt_function(
                handler,
                irq,
                IstIndex::NormalInterrupt,
                InterruptManager::irq_to_index(irq.unwrap()),
                0,
            )
        {
            pr_err!("Cannot setup interrupt.");
            return;
        }

        /* Clear Interrupt Disable and enable I/O Space */
        pci_manager.write_config_address_register(bus, device, function, Self::PCI_CMD);
        pci_manager.write_config_data_register(
            (pci_manager.read_config_data_register() | Self::IO_SPACE_ENABLE)
                & !(Self::INTERRUPT_DISABLE),
        );

        /* Set SMBus Host Enable Bit */
        pci_manager.write_config_address_register(bus, device, function, Self::HOST_CONFIG);
        pci_manager.write_config_data_register(
            pci_manager.read_config_data_register() | Self::SMBUS_HOST_ENABLE,
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
    }
}

extern "C" fn smbus_handler() {
    pr_info!("Interrupted from SMBus.(Currently, do nothing.)");
    get_cpu_manager_cluster().interrupt_manager.send_eoi();
}
