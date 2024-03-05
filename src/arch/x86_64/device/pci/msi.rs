//!
//! Message Signal Interrupt
//!

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum MsiDeliveryMode {
    Fixed = 0b000,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum MsiTriggerMode {
    Edge = 0,
    Level = 1,
}

use crate::kernel::drivers::pci::PciDevice;
use crate::kernel::manager_cluster::get_kernel_manager_cluster;

pub fn setup_msi(
    pci_dev: &PciDevice,
    destination_id: u8,
    trigger_mode: MsiTriggerMode,
    is_assert: bool,
    delivery_mode: MsiDeliveryMode,
    vector: u16,
) -> Result<(), ()> {
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

    let message_address = 0xfee00000u32 | ((destination_id as u32) << 12);
    let message_data = ((trigger_mode as u32) << 15)
        | ((is_assert as u32) << 14)
        | ((delivery_mode as u32) << 8)
        | (vector as u32);
    get_kernel_manager_cluster().pci_manager.write_data(
        pci_dev,
        usable_capability + 0x4,
        message_address,
    )?;
    let data_register_offset = if (message_control & (1 << (16 + 7))) != 0 {
        0x0C
    } else {
        0x08
    };
    get_kernel_manager_cluster().pci_manager.write_data(
        pci_dev,
        usable_capability + data_register_offset,
        message_data,
    )?;
    get_kernel_manager_cluster().pci_manager.write_data(
        pci_dev,
        usable_capability,
        message_control | (1 << 16),
    )?;
    Ok(())
}
