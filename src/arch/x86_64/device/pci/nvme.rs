//!
//! NVMe Arch Depend
//!

use crate::arch::target_arch::device::pci::msi::{setup_msi, MsiDeliveryMode, MsiTriggerMode};
use crate::arch::target_arch::interrupt::{InterruptionIndex, IstIndex};

use crate::kernel::drivers::pci::PciDevice;
use crate::kernel::manager_cluster::get_cpu_manager_cluster;

pub fn setup_interrupt(pci_dev: &PciDevice) -> Result<(), ()> {
    make_device_interrupt_handler!(handler, nvme_handler);
    get_cpu_manager_cluster()
        .interrupt_manager
        .set_device_interrupt_function(
            handler,
            None,
            IstIndex::NormalInterrupt,
            InterruptionIndex::Nvme as u16,
            0,
            true,
        );
    setup_msi(
        pci_dev,
        get_cpu_manager_cluster()
            .interrupt_manager
            .get_local_apic_manager()
            .get_apic_id() as u8,
        MsiTriggerMode::Level,
        true,
        MsiDeliveryMode::Fixed,
        InterruptionIndex::Nvme as u16,
    )?;

    return Ok(());
}

#[inline(never)]
fn nvme_handler() {
    pr_info!("NVMe Interrupt");

    get_cpu_manager_cluster()
        .interrupt_manager
        .send_eoi_level_trigger(InterruptionIndex::Nvme as _);
}
