//!
//! The functions for initialization
//!
//! This module contains initialization functions which is not depend on arch.
//!

use crate::arch::target_arch::device::pci::ArchDependPciManager;
use crate::kernel::drivers::acpi::device::AcpiDeviceManager;
use crate::kernel::drivers::acpi::table::mcfg::McfgManager;
use crate::kernel::drivers::acpi::AcpiManager;
use crate::kernel::drivers::pci::PciManager;
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::task_manager::run_queue::RunQueue;

/// Init application processor's TaskManager
///
///
pub fn init_task_ap(idle_task: fn() -> !) {
    let mut run_queue = RunQueue::new();
    run_queue.init().expect("Failed to init RunQueue");

    get_kernel_manager_cluster()
        .task_manager
        .init_idle(idle_task, &mut run_queue);
    get_cpu_manager_cluster().run_queue = run_queue;
}

/// Init Work Queue
pub fn init_work_queue() {
    get_cpu_manager_cluster()
        .work_queue
        .init_cpu_work_queue(&mut get_kernel_manager_cluster().task_manager);
}

/// Init AcpiManager without parsing AML
///
/// This function initializes ACPI Manager.
/// ACPI Manager will parse some tables and return.
/// If succeeded, this will move it into kernel_manager_cluster.
pub fn init_acpi_early(rsdp_ptr: usize) -> bool {
    let mut acpi_manager = AcpiManager::new();
    let mut device_manager = AcpiDeviceManager::new();
    let set_manger = |a: AcpiManager, d: AcpiDeviceManager| {
        init_struct!(get_kernel_manager_cluster().acpi_manager, Mutex::new(a));
        init_struct!(get_kernel_manager_cluster().acpi_device_manager, d);
    };

    if !acpi_manager.init(rsdp_ptr, &mut device_manager) {
        pr_warn!("Cannot init ACPI.");
        set_manger(acpi_manager, device_manager);
        return false;
    }
    if let Some(e) = acpi_manager.create_acpi_event_manager() {
        init_struct!(get_kernel_manager_cluster().acpi_event_manager, e);
    } else {
        pr_err!("Cannot init ACPI Event Manager");
        set_manger(acpi_manager, device_manager);
        return false;
    }
    set_manger(acpi_manager, device_manager);
    return true;
}

/// Init AcpiManager and AcpiEventManager with parsing AML
///
/// This function will setup some devices like power button.
/// They will call malloc, therefore this function should be called after init of kernel_memory_manager
pub fn init_acpi_later() -> bool {
    let mut acpi_manager = get_kernel_manager_cluster().acpi_manager.lock().unwrap();
    if !acpi_manager.is_available() {
        pr_info!("ACPI is not available.");
        return true;
    }
    if !acpi_manager.setup_aml_interpreter() {
        pr_err!("Cannot setup ACPI AML Interpreter.");
        return false;
    }
    if !super::device::acpi::setup_interrupt(&acpi_manager) {
        pr_err!("Cannot setup ACPI interrupt.");
        return false;
    }
    if !acpi_manager.setup_acpi_devices(&mut get_kernel_manager_cluster().acpi_device_manager) {
        pr_err!("Cannot setup ACPI devices.");
        return false;
    }
    if !acpi_manager.initialize_all_devices() {
        pr_err!("Cannot evaluate _STA/_INI methods.");
        return false;
    }
    get_kernel_manager_cluster()
        .acpi_event_manager
        .init_event_registers();
    if !acpi_manager.enable_acpi() {
        pr_err!("Cannot enable ACPI.");
        return false;
    }
    if !acpi_manager.enable_power_button(&mut get_kernel_manager_cluster().acpi_event_manager) {
        pr_err!("Cannot enable power button.");
        return false;
    }
    get_kernel_manager_cluster()
        .acpi_event_manager
        .enable_gpes();
    return true;
}

/// Init PciManager without scanning all bus
///
/// This function should be called before `init_acpi_later`.
pub fn init_pci_early() -> bool {
    let acpi_manager = get_kernel_manager_cluster().acpi_manager.lock().unwrap();

    let pci_manager;
    if acpi_manager.is_available() {
        if let Some(mcfg_manager) = acpi_manager
            .get_table_manager()
            .get_table_manager::<McfgManager>()
        {
            drop(acpi_manager);
            pci_manager = PciManager::new_ecam(mcfg_manager);
        } else {
            pci_manager = PciManager::new_arch_depend(ArchDependPciManager::new());
        }
    } else {
        pci_manager = PciManager::new_arch_depend(ArchDependPciManager::new());
    }
    init_struct!(get_kernel_manager_cluster().pci_manager, pci_manager);
    if let Err(e) = get_kernel_manager_cluster().pci_manager.build_device_tree() {
        pr_err!("Failed to build PCI device tree: {:?}", e);
        return false;
    }
    return true;
}

/// Init PciManager with scanning all bus
pub fn init_pci_later() -> bool {
    get_kernel_manager_cluster().pci_manager.setup_devices();
    return true;
}

/// Init global timer
pub fn init_global_timer() {
    init_struct!(
        get_kernel_manager_cluster().global_timer_manager,
        GlobalTimerManager::new()
    );
}

/// Initialize Block Device Manager and File System Manager
///
/// This function must be called before calling device scan functions.
pub fn init_block_devices_and_file_system_early() {
    init_struct!(
        get_kernel_manager_cluster().block_device_manager,
        BlockDeviceManager::new()
    );
    init_struct!(
        get_kernel_manager_cluster().file_manager,
        FileManager::new()
    );
}

/// Initialize Network Manager
///
/// This function must be called before calling device scan functions.
pub fn init_network_manager_early() {
    get_kernel_manager_cluster().network_manager.init();
}

/// Search partitions and try to mount them
///
/// This function will be called after completing the device initializations.
pub fn init_block_devices_and_file_system_later() {
    for i in 0..get_kernel_manager_cluster()
        .block_device_manager
        .get_number_of_devices()
    {
        get_kernel_manager_cluster()
            .file_manager
            .detect_partitions(i);
    }
}
