//!
//! System Management Bus for Intel Chipset
//!

use crate::arch::target_arch::device::cpu::{in_byte, out_byte};
use crate::arch::target_arch::interrupt::IstIndex;

use crate::kernel::drivers::pci::PciManager;
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};

pub fn setup_sm_bus(pci_manager: &PciManager, bus: u8, device: u8, function: u8, header_type: u8) {
    if header_type != 0 {
        pr_err!("Invalid header type: {}", header_type);
        return;
    }
    pci_manager.write_config_address_register(bus, device, function, 0x04);
    let status = pci_manager.read_config_data_register() >> 16;
    if (status & (1 << 3)) != 0 {
        pr_info!("SMBus interrupt is pending.");
    }
    pci_manager.write_config_address_register(bus, device, function, 0x20);
    let smbus_base_address = pci_manager.read_config_data_register();
    pr_info!("Base address of SMBus: {:#X}", smbus_base_address & !0b1);
    pci_manager.write_config_address_register(bus, device, function, 0x3c);

    let interrupt_pin = ((pci_manager.read_config_data_register() >> 8) & 0xFF) as u8;
    pr_info!("Interrupt Pin: {:#X}", interrupt_pin);
    let irq = get_kernel_manager_cluster()
        .acpi_manager
        .lock()
        .unwrap()
        .search_int_number_with_evaluation_aml(bus, device, interrupt_pin);
    if irq.is_none() {
        pr_err!("Cannot detect irq.");
        return;
    }
    pr_info!("IRQ: {}", irq.unwrap());

    make_device_interrupt_handler!(handler, smbus_handler);
    get_cpu_manager_cluster()
        .interrupt_manager
        .set_device_interrupt_function(handler, irq, IstIndex::NormalInterrupt, 0x50, 0);

    pci_manager.write_config_address_register(bus, device, function, 0x40);
    let host_cfg_status = pci_manager.read_config_data_register();
    pci_manager.write_config_data_register(host_cfg_status | 1);
    pci_manager.write_config_address_register(bus, device, function, 0x04);
    pci_manager.write_config_data_register(1);

    unsafe {
        out_byte(
            smbus_base_address as _,
            in_byte(smbus_base_address as _) | 1,
        );
        out_byte(smbus_base_address as u16 + 0x11, 1);
        out_byte(smbus_base_address as u16 + 0x10, 1);
    }
}

extern "C" fn smbus_handler() {
    get_cpu_manager_cluster().interrupt_manager.send_eoi();
    pr_info!("SMBus!!");
}
