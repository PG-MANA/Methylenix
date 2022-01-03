//!
//! NVMe Arch Depend
//!

use crate::arch::target_arch::interrupt::{InterruptionIndex, IstIndex};

use crate::kernel::drivers::pci::PciDevice;
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};

pub fn setup_interrupt(pci_dev: &PciDevice) -> Result<(), ()> {
    setup_msi(pci_dev)?;
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
    return Ok(());
}

fn setup_msi(pci_dev: &PciDevice) -> Result<(), ()> {
    let capability = get_kernel_manager_cluster()
        .pci_manager
        .read_data(pci_dev, 0x34, 1)?;
    pr_debug!("Capability: {:#X}", capability);
    let mut usable_capability = capability;
    loop {
        let d =
            get_kernel_manager_cluster()
                .pci_manager
                .read_data(pci_dev, usable_capability, 4)?;

        if (d & (1 << 16)) != 0 {
            pr_debug!("Capability Pointer: {:#X} is in use.", usable_capability);
            usable_capability = (d >> 8) & (u8::MAX as u32);
            if usable_capability == 0 {
                pr_err!("No usable capability pointer");
                return Err(());
            }
            continue;
        }
        let message_address = 0xfee00000u32
            | (get_cpu_manager_cluster()
                .interrupt_manager
                .get_local_apic_manager()
                .get_apic_id()
                << 12);
        let message_data = 0xc000u32 | (InterruptionIndex::Nvme as u32);
        get_kernel_manager_cluster().pci_manager.write_data(
            pci_dev,
            usable_capability + 0x4,
            message_address,
        )?;
        get_kernel_manager_cluster().pci_manager.write_data(
            pci_dev,
            usable_capability + 0xC,
            message_data,
        )?;
        get_kernel_manager_cluster().pci_manager.write_data(
            pci_dev,
            usable_capability + 0x0,
            d | 1,
        )?;
        return Ok(());
    }
}

#[inline(never)]
fn nvme_handler() {
    pr_info!("NVMe Interrupt");

    get_cpu_manager_cluster()
        .interrupt_manager
        .send_eoi_level_trigger(InterruptionIndex::Nvme as _);
}
