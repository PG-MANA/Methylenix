//!
//! Init codes
//!
//! this module including init codes for device, memory, and task system.
//! This module is called by boot function.

pub mod multiboot;
use arch::target_arch::context::ContextManager;
use arch::target_arch::device::local_apic_timer::LocalApicTimer;
use arch::target_arch::device::pit::PitManager;
use arch::target_arch::device::{cpu, pic};
use arch::target_arch::interrupt::{InterruptManager, InterruptionIndex};
use arch::target_arch::paging::PAGE_SIZE;

use kernel::drivers::acpi::AcpiManager;
use kernel::manager_cluster::get_kernel_manager_cluster;
use kernel::memory_manager::data_type::{Address, MSize};
use kernel::sync::spin_lock::Mutex;
use kernel::task_manager::TaskManager;

/// Memory Areas for PhysicalMemoryManager
static mut MEMORY_FOR_PHYSICAL_MEMORY_MANAGER: [u8; PAGE_SIZE * 2] = [0; PAGE_SIZE * 2];

/// Init TaskManager
///
///
pub fn init_task(
    system_cs: u16,
    user_cs: u16,
    user_ss: u16,
    main_process: fn() -> !,
    idle_task: fn() -> !,
) {
    let mut context_manager = ContextManager::new();
    context_manager.init(
        system_cs as usize,
        0, /*is it ok?*/
        user_cs as usize,
        user_ss as usize,
    );

    let mut kernel_memory_alloc_manager = get_kernel_manager_cluster()
        .kernel_memory_alloc_manager
        .lock()
        .unwrap();
    let memory_manager = &get_kernel_manager_cluster().memory_manager;

    let stack_for_init = kernel_memory_alloc_manager
        .vmalloc(
            ContextManager::DEFAULT_STACK_SIZE_OF_SYSTEM.into(),
            ContextManager::STACK_ALIGN_ORDER.into(),
            memory_manager,
        )
        .unwrap()
        + MSize::from(ContextManager::DEFAULT_STACK_SIZE_OF_SYSTEM);
    let stack_for_idle = kernel_memory_alloc_manager
        .vmalloc(
            ContextManager::DEFAULT_STACK_SIZE_OF_SYSTEM.into(),
            ContextManager::STACK_ALIGN_ORDER.into(),
            memory_manager,
        )
        .unwrap()
        + MSize::from(ContextManager::DEFAULT_STACK_SIZE_OF_SYSTEM);
    drop(kernel_memory_alloc_manager);

    let context_data_for_init = context_manager.create_system_context(
        main_process as *const fn() as usize,
        stack_for_init.to_usize(),
        unsafe { cpu::get_cr3() },
    );
    let context_data_for_idle = context_manager.create_system_context(
        idle_task as *const fn() as usize,
        stack_for_idle.to_usize(),
        unsafe { cpu::get_cr3() },
    );

    get_kernel_manager_cluster().task_manager = TaskManager::new();
    get_kernel_manager_cluster()
        .task_manager
        .init(context_manager);
    get_kernel_manager_cluster()
        .task_manager
        .create_init_process(context_data_for_init, context_data_for_idle);
}

/// Init InterruptManager
///
/// This function disables 8259 PIC and init InterruptManager
pub fn init_interrupt(kernel_selector: u16) {
    pic::disable_8259_pic();
    let mut interrupt_manager = InterruptManager::new();
    interrupt_manager.init(kernel_selector);
    get_kernel_manager_cluster().interrupt_manager = Mutex::new(interrupt_manager);
}

///Init AcpiManager
pub fn init_acpi(rsdp_ptr: usize) -> Option<AcpiManager> {
    use core::str;

    let mut acpi_manager = AcpiManager::new();
    if !acpi_manager.init(rsdp_ptr) {
        pr_warn!("Cannot init ACPI.");
        return None;
    }
    pr_info!(
        "OEM ID:{}",
        str::from_utf8(&acpi_manager.get_oem_id().unwrap_or([0; 6])).unwrap_or("NOT FOUND")
    );
    Some(acpi_manager)
}

/// Init Timer
///
/// This function tries to set up LocalApicTimer.
/// If TSC-Deadline mode is usable, this will enable it and return.
/// Otherwise, this will calculate the frequency of the Local APIC Timer with ACPI PM Timer or
/// PIT.(ACPI PM Timer is prioritized.)
/// After that, this registers the timer to InterruptManager.
pub fn init_timer(acpi_manager: Option<&AcpiManager>) -> LocalApicTimer {
    /* This function assumes that interrupt is not enabled */
    /* This function does not enable interrupt */
    let mut local_apic_timer = LocalApicTimer::new();
    local_apic_timer.init();
    if local_apic_timer.enable_deadline_mode(
        InterruptionIndex::LocalApicTimer as u16,
        get_kernel_manager_cluster()
            .interrupt_manager
            .lock()
            .unwrap()
            .get_local_apic_manager(),
    ) {
        pr_info!("Using Local APIC TSC Deadline Mode");
    } else if let Some(pm_timer) = acpi_manager
        .unwrap_or(&AcpiManager::new())
        .get_xsdt_manager()
        .get_fadt_manager()
        .get_acpi_pm_timer()
    {
        pr_info!("Using ACPI PM Timer to calculate frequency of Local APIC Timer.");
        local_apic_timer.set_up_interruption(
            InterruptionIndex::LocalApicTimer as u16,
            get_kernel_manager_cluster()
                .interrupt_manager
                .lock()
                .unwrap()
                .get_local_apic_manager(),
            &pm_timer,
        );
    } else {
        pr_info!("Using PIT to calculate frequency of Local APIC Timer.");
        let mut pit = PitManager::new();
        pit.init();
        local_apic_timer.set_up_interruption(
            InterruptionIndex::LocalApicTimer as u16,
            get_kernel_manager_cluster()
                .interrupt_manager
                .lock()
                .unwrap()
                .get_local_apic_manager(),
            &pit,
        );
        pit.stop_counting();
    }
    /*setup IDT*/
    make_device_interrupt_handler!(
        local_apic_timer_handler,
        LocalApicTimer::local_apic_timer_handler
    );
    get_kernel_manager_cluster()
        .interrupt_manager
        .lock()
        .unwrap()
        .set_device_interrupt_function(
            local_apic_timer_handler,
            None,
            InterruptionIndex::LocalApicTimer as u16,
            0,
        );
    local_apic_timer
}
