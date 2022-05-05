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
use crate::kernel::memory_manager::data_type::{Address, MSize, MemoryPermissionFlags, PAddress};

use crate::{free_pages, io_remap};

pub fn setup_msi_or_msi_x(
    pci_dev: &PciDevice,
    handler: fn(usize) -> bool,
    priority: Option<u8>,
    is_level_trigger: bool,
) -> Result<usize, ()> {
    if let Ok(a) = setup_msi(pci_dev, handler, priority, is_level_trigger) {
        return Ok(a);
    }

    if let Ok(a) = setup_msi_x(pci_dev, handler, priority, is_level_trigger) {
        return Ok(a);
    }
    return Err(());
}

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

pub fn setup_msi_x(
    pci_dev: &PciDevice,
    handler: fn(usize) -> bool,
    priority: Option<u8>,
    is_level_trigger: bool,
) -> Result<usize, ()> {
    let capability = get_kernel_manager_cluster()
        .pci_manager
        .read_data(pci_dev, 0x34, 1)?;
    pr_debug!("Capability: {:#X}", capability);
    let mut msi_x_capability = if capability == 0 { 0x80}else{capability};
    let mut message_control: u32;
    loop {
        message_control =
            get_kernel_manager_cluster()
                .pci_manager
                .read_data(pci_dev, msi_x_capability, 4)?;

        if (message_control & 0xff) == 0x0B {
            break;
        }
        msi_x_capability = (message_control >> 8) & (u8::MAX as u32);
        if msi_x_capability == 0 {
            pr_err!("No usable capability pointer");
            return Err(());
        }
    }

    let table_offset =
        get_kernel_manager_cluster()
            .pci_manager
            .read_data(pci_dev, msi_x_capability + 0x04, 4)?;
    let bir = table_offset & 0b111;
    let table_offset = table_offset & !0b111;
    pr_debug!("BIR: {bir}");

    let msi_x_table_address = get_kernel_manager_cluster()
        .pci_manager
        .read_base_address_register(pci_dev, bir as u8)?;
    let msi_x_table_address = (msi_x_table_address & !0b1111) as usize
        | if ((msi_x_table_address >> 1) & 0b11) == 0b10 {
            (get_kernel_manager_cluster()
                .pci_manager
                .read_base_address_register(pci_dev, bir as u8 + 1)? as usize)
                << 32
        } else {
            0
        };
    let number_of_entries = ((msi_x_capability >> 16) & ((11 << 1) - 1)) + 1;

    pr_debug!(
        "MSI-X Address: {:#X}(Number of entries: {number_of_entries})",
        msi_x_table_address
    );
    let info = get_cpu_manager_cluster()
        .interrupt_manager
        .setup_msi_interrupt(handler, priority, is_level_trigger)?;

    let msi_x_table_address = match io_remap!(
        PAddress::new(msi_x_table_address),
        MSize::new((number_of_entries as usize) << 4),
        MemoryPermissionFlags::data()
    ) {
        Ok(a) => a,
        Err(e) => {
            pr_debug!("Failed to map MSI-X table: {:?}", e);
            return Err(());
        }
    };
    let msi_x_target_address = msi_x_table_address.to_usize() + ((table_offset as usize) << 4);

    unsafe {
        *(msi_x_target_address as *mut u32) = (info.message_address & u32::MAX as u64) as u32;
        *((msi_x_target_address + 4) as *mut u32) = (info.message_address >> u32::BITS) as u32;
        *((msi_x_target_address + 8) as *mut u32) = (info.message_data & u32::MAX as u64) as u32;
        *((msi_x_target_address + 12) as *mut u32) = 0;
    }
    let _ = free_pages!(msi_x_table_address);

    get_kernel_manager_cluster().pci_manager.write_data(
        pci_dev,
        msi_x_capability,
        (message_control & !(1 << 30)) | (1 << 31),
    )?;

    return Ok(info.interrupt_id);
}
