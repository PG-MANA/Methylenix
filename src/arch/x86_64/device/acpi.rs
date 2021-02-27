//!
//! Arch-depended ACPI support
//!

use crate::kernel::drivers::acpi::event::AcpiEventManager;
use crate::kernel::drivers::acpi::AcpiManager;
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::task_manager::work_queue::WorkList;

pub fn setup_interrupt(acpi_manager: &AcpiManager) -> bool {
    let irq = acpi_manager.get_fadt_manager().get_sci_int();
    make_device_interrupt_handler!(handler, acpi_event_handler);
    get_cpu_manager_cluster()
        .interrupt_manager
        .lock()
        .unwrap()
        .set_device_interrupt_function(handler, Some(irq as u8), None, 0x20 + irq, 0);

    return true;
}

extern "C" fn acpi_event_handler() {
    if let Some(acpi_event) = get_kernel_manager_cluster()
        .acpi_event_manager
        .find_occurred_fixed_event()
    {
        let work = WorkList::new(AcpiEventManager::acpi_fixed_event_worker, acpi_event as _);
        get_cpu_manager_cluster().work_queue.add_work(work);
        if !get_kernel_manager_cluster()
            .acpi_event_manager
            .reset_fixed_event_status(acpi_event)
        {
            pr_err!("Cannot reset flag: {:?}", acpi_event);
        }
    } else {
        pr_err!("Unknown ACPI Event");
    }

    if let Ok(interrupt_manager) = get_kernel_manager_cluster()
        .boot_strap_cpu_manager
        .interrupt_manager
        .try_lock()
    {
        interrupt_manager.send_eoi();
    }
}
