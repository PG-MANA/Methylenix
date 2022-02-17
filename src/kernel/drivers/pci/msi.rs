//!
//! Message Signaled Interrupts
//!

#[derive(Clone)]
pub struct MsiInfo {
    pub message_address: u64,
    pub message_data: u64,
    pub interrupt_id: usize,
}

use crate::kernel::drivers::pci::PciDevice;
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};

pub fn setup_msi(
    pci_dev: &PciDevice,
    handler: fn(usize) -> bool,
    priority: Option<u8>,
    is_level_trigger: bool,
) -> Result<usize, ()> {
    let capability = get_kernel_manager_cluster()
        .pci_manager
        .read_data(pci_dev, 0x34, 1)?;
    pr_debug!("Capability: {:#X}", capability);
    let mut usable_capability = capability;
    let mut message_control: u32;
    loop {
        message_control =
            get_kernel_manager_cluster()
                .pci_manager
                .read_data(pci_dev, usable_capability, 4)?;

        if (message_control & 0xff) != 0x05 {
            pr_debug!("Capability ID is not for MSI");
        } else if (message_control & (1 << 16)) != 0 {
            pr_debug!("Capability Pointer: {:#X} is in use.", usable_capability);
        } else {
            break;
        }
        usable_capability = (message_control >> 8) & (u8::MAX as u32);
        if usable_capability == 0 {
            pr_err!("No usable capability pointer");
            return Err(());
        }
    }

    let info = get_cpu_manager_cluster()
        .interrupt_manager
        .setup_msi_interrupt(handler, priority, is_level_trigger)?;
    get_kernel_manager_cluster().pci_manager.write_data(
        pci_dev,
        usable_capability + 0x4,
        (info.message_address & u32::MAX as u64) as u32,
    )?;

    let message_address_high = (info.message_address >> 32) as u32;
    let data_register_offset = if (message_control & (1 << (16 + 7))) != 0 {
        get_kernel_manager_cluster().pci_manager.write_data(
            pci_dev,
            usable_capability + 0x8,
            message_address_high,
        )?;
        0x0C
    } else {
        if message_address_high != 0 {
            pr_debug!("MSI message address is not 64bit.");
            return Err(());
        }
        0x08
    };
    get_kernel_manager_cluster().pci_manager.write_data(
        pci_dev,
        usable_capability + data_register_offset,
        (info.message_data & u32::MAX as u64) as u32,
    )?;
    get_kernel_manager_cluster().pci_manager.write_data(
        pci_dev,
        usable_capability + 0x0,
        message_control | (1 << 16),
    )?;
    return Ok(info.interrupt_id);
}
