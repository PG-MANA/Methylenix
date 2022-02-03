//!
//! NVMe Arch Depend
//!

use crate::arch::target_arch::device::pci::msi::{setup_msi, MsiDeliveryMode, MsiTriggerMode};
use crate::arch::target_arch::interrupt::InterruptIndex;

use crate::kernel::drivers::pci::PciDevice;
use crate::kernel::manager_cluster::get_cpu_manager_cluster;

pub fn setup_interrupt(pci_dev: &PciDevice) -> Result<(), ()> {
    get_cpu_manager_cluster()
        .interrupt_manager
        .set_device_interrupt_function(nvme_handler, None, InterruptIndex::Nvme as usize, 0, true);
    setup_msi(
        pci_dev,
        get_cpu_manager_cluster()
            .interrupt_manager
            .get_local_apic_manager()
            .get_apic_id() as u8,
        MsiTriggerMode::Level,
        true,
        MsiDeliveryMode::Fixed,
        InterruptIndex::Nvme as u16,
    )?;

    return Ok(());
}

fn nvme_handler(index: usize) {
    pr_info!("NVMe Interrupt: {:#X}", index);

    get_cpu_manager_cluster()
        .interrupt_manager
        .send_eoi_level_trigger(index as _);
}
