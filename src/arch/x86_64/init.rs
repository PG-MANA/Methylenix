//!
//! Init codes
//!
//! this module including init codes for device, memory, and task system.
//! This module is called by boot function.

pub mod multiboot;

use crate::arch::target_arch::context::ContextManager;
use crate::arch::target_arch::device::local_apic_timer::LocalApicTimer;
use crate::arch::target_arch::device::pit::PitManager;
use crate::arch::target_arch::device::{cpu, pic};
use crate::arch::target_arch::interrupt::{InterruptManager, InterruptionIndex};
use crate::arch::target_arch::paging::PAGE_SIZE_USIZE;

use crate::arch::x86_64::context::context_data::ContextData;
use crate::kernel::drivers::acpi::AcpiManager;
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::sync::spin_lock::Mutex;
use crate::kernel::task_manager::TaskManager;

/// Memory Areas for PhysicalMemoryManager
static mut MEMORY_FOR_PHYSICAL_MEMORY_MANAGER: [u8; PAGE_SIZE_USIZE * 2] = [0; PAGE_SIZE_USIZE * 2];

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
    context_manager.init(system_cs, 0 /*is it ok?*/, user_cs, user_ss);

    let create_context = |c: &ContextManager, entry_function: fn() -> !| -> ContextData {
        match c.create_system_context(entry_function as *const fn() as usize, None, unsafe {
            cpu::get_cr3()
        }) {
            Ok(m) => m,
            Err(e) => panic!("Cannot create a ContextData: {:?}", e),
        }
    };

    let context_for_main = create_context(&context_manager, main_process);
    let context_for_idle = create_context(&context_manager, idle_task);

    get_kernel_manager_cluster().task_manager = TaskManager::new();
    get_kernel_manager_cluster()
        .task_manager
        .init(context_manager);
    get_kernel_manager_cluster()
        .task_manager
        .create_kernel_process(context_for_main, context_for_idle);
}

/// Init SoftInterrupt
pub fn init_interrupt_work_queue_manager() {
    get_kernel_manager_cluster()
        .soft_interrupt_manager
        .init(&mut get_kernel_manager_cluster().task_manager);
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

    /* setup IDT */
    make_context_switch_interrupt_handler!(
        local_apic_timer_handler,
        LocalApicTimer::local_apic_timer_handler
    );

    get_kernel_manager_cluster()
        .interrupt_manager
        .lock()
        .unwrap()
        .set_ist(1, 0x4000.into());

    get_kernel_manager_cluster()
        .interrupt_manager
        .lock()
        .unwrap()
        .set_device_interrupt_function(
            local_apic_timer_handler,
            None,
            Some(1),
            InterruptionIndex::LocalApicTimer as u16,
            0,
        );
    local_apic_timer
}
