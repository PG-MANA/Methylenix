//!
//! NVMe Arch Depend
//!

use crate::arch::target_arch::device::pci::msi::{setup_msi, MsiDeliveryMode, MsiTriggerMode};

use crate::kernel::drivers::device::nvme::NvmeManager;
use crate::kernel::drivers::pci::PciDevice;
use crate::kernel::manager_cluster::get_cpu_manager_cluster;

use alloc::collections::LinkedList;

pub fn setup_interrupt(pci_dev: &PciDevice, nvme_manager: &mut NvmeManager) -> Result<(), ()> {
    let vector = get_cpu_manager_cluster()
        .interrupt_manager
        .set_device_interrupt_function(nvme_handler, None, None, 0, true)?;
    setup_msi(
        pci_dev,
        get_cpu_manager_cluster()
            .interrupt_manager
            .get_local_apic_manager()
            .get_apic_id() as u8,
        MsiTriggerMode::Level,
        true,
        MsiDeliveryMode::Fixed,
        vector as u16,
    )?;

    unsafe { NVME_LIST.push_back((vector, nvme_manager as *mut _)) };
    return Ok(());
}

static mut NVME_LIST: LinkedList<(usize, *mut NvmeManager)> = LinkedList::new();

fn nvme_handler(index: usize) {
    if let Some(nvme) = unsafe {
        NVME_LIST
            .iter()
            .find(|x| (**x).0 == index)
            .and_then(|x| Some(x.1.clone()))
    } {
        unsafe { &mut *(nvme) }.interrupt_handler();
    } else {
        pr_err!("Unknown NVMe Device");
    }

    get_cpu_manager_cluster()
        .interrupt_manager
        .send_eoi_level_trigger(index as _);
}
