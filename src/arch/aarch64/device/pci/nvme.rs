//!
//! NVMe Arch Depend
//!

//use crate::arch::target_arch::device::pci::msi::{setup_msi, MsiDeliveryMode, MsiTriggerMode};

use crate::kernel::drivers::device::nvme::NvmeManager;
use crate::kernel::drivers::pci::PciDevice;
use alloc::collections::LinkedList;

pub fn setup_interrupt(_pci_dev: &PciDevice, _nvme_manager: &mut NvmeManager) -> Result<(), ()> {
    return Err(());
}

#[allow(dead_code)]
static mut NVME_LIST: LinkedList<(usize, *mut NvmeManager)> = LinkedList::new();

#[allow(dead_code)]
fn nvme_handler(index: usize) -> bool {
    if let Some(nvme) = unsafe {
        NVME_LIST
            .iter()
            .find(|x| (**x).0 == index)
            .and_then(|x| Some(x.1.clone()))
    } {
        unsafe { &mut *(nvme) }.interrupt_handler();
        true
    } else {
        pr_err!("Unknown NVMe Device");
        false
    }
}
