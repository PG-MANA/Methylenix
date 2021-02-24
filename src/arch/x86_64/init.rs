//!
//! Init codes
//!
//! This module including init codes for device, memory, and task system.
//! This module is called by boot function.

pub mod multiboot;

use crate::arch::target_arch::context::context_data::ContextData;
use crate::arch::target_arch::context::ContextManager;
use crate::arch::target_arch::device::io_apic::IoApicManager;
use crate::arch::target_arch::device::local_apic_timer::LocalApicTimer;
use crate::arch::target_arch::device::pit::PitManager;
use crate::arch::target_arch::device::{cpu, pic};
use crate::arch::target_arch::interrupt::{InterruptManager, InterruptionIndex};
use crate::arch::target_arch::paging::{PAGE_SHIFT, PAGE_SIZE_USIZE};

use crate::kernel::drivers::acpi::AcpiManager;
use crate::kernel::manager_cluster::{
    get_cpu_manager_cluster, get_kernel_manager_cluster, CpuManagerCluster,
};
use crate::kernel::memory_manager::data_type::{
    Address, MSize, MemoryOptionFlags, MemoryPermissionFlags, VAddress,
};
use crate::kernel::ptr_linked_list::PtrLinkedListNode;
use crate::kernel::sync::spin_lock::Mutex;
use crate::kernel::task_manager::run_queue_manager::RunQueueManager;
use crate::kernel::task_manager::TaskManager;
use crate::kernel::timer_manager::Timer;

use core::sync::atomic::AtomicBool;

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
    context_manager.init(
        system_cs,
        0, /*is it ok?*/
        user_cs,
        user_ss,
        unsafe { cpu::get_cr3() },
    );

    let create_context = |c: &ContextManager, entry_function: fn() -> !| -> ContextData {
        match c.create_system_context(entry_function as *const fn() as usize, None) {
            Ok(m) => m,
            Err(e) => panic!("Cannot create a ContextData: {:?}", e),
        }
    };
    let context_for_main = create_context(&context_manager, main_process);

    get_cpu_manager_cluster().run_queue_manager = RunQueueManager::new();
    get_cpu_manager_cluster().run_queue_manager.init();
    get_kernel_manager_cluster().task_manager = TaskManager::new();
    let main_thread = get_kernel_manager_cluster()
        .task_manager
        .init(context_manager, context_for_main);
    let idle_thread = get_kernel_manager_cluster()
        .task_manager
        .create_kernel_thread(
            idle_task as *const fn() -> !,
            Some(MSize::new(0x1000)),
            i8::MIN,
        )
        .unwrap();
    get_cpu_manager_cluster()
        .run_queue_manager
        .add_thread(main_thread);
    get_cpu_manager_cluster()
        .run_queue_manager
        .add_thread(idle_thread);
}

/// Init application processor's TaskManager
///
///
pub fn init_task_ap(idle_task: fn() -> !) {
    get_cpu_manager_cluster().run_queue_manager = RunQueueManager::new();
    get_cpu_manager_cluster().run_queue_manager.init();
    let idle_thread = get_kernel_manager_cluster()
        .task_manager
        .create_kernel_thread(
            idle_task as *const fn() -> !,
            Some(MSize::new(0x1000)),
            i8::MIN,
        )
        .unwrap();
    get_cpu_manager_cluster()
        .run_queue_manager
        .add_thread(idle_thread);
}

/// Init SoftInterrupt
pub fn init_interrupt_work_queue_manager() {
    get_cpu_manager_cluster()
        .work_queue_manager
        .init(&mut get_kernel_manager_cluster().task_manager);
}

/// Init InterruptManager
///
/// This function disables 8259 PIC and init InterruptManager
pub fn init_interrupt(kernel_selector: u16) {
    pic::disable_8259_pic();

    let mut interrupt_manager = InterruptManager::new();
    interrupt_manager.init(kernel_selector);
    get_kernel_manager_cluster()
        .boot_strap_cpu_manager
        .interrupt_manager = Mutex::new(interrupt_manager);

    let mut io_apic_manager = IoApicManager::new();
    io_apic_manager.init();
    get_kernel_manager_cluster()
        .arch_depend_data
        .io_apic_manager = Mutex::new(io_apic_manager);
}

/// Init AcpiManager without parsing AML
///
/// This function initializes ACPI Manager.
/// ACPI Manager will parse some tables and return.
/// If succeeded, this will move it into kernel_manager_cluster.
pub fn init_acpi_early(rsdp_ptr: usize) -> bool {
    let mut acpi_manager = AcpiManager::new();
    if !acpi_manager.init(rsdp_ptr) {
        pr_warn!("Cannot init ACPI.");
        return false;
    }
    if !acpi_manager.init_acpi_event_manager(&mut get_kernel_manager_cluster().acpi_event_manager) {
        pr_err!("Cannot init ACPI Event Manager");
        return false;
    }

    if let Some(oem) = acpi_manager.get_oem_id() {
        pr_info!("OEM ID: {}", oem,);
    }

    get_kernel_manager_cluster().acpi_manager = Mutex::new(acpi_manager);
    return true;
}

/// Init AcpiManager and AcpiEventManager with parsing AML
///
/// This function will setup some devices like power button.
/// They will call malloc, therefore this function should be called after init of memory_manager
pub fn init_acpi_later() -> bool {
    let mut acpi_manager = get_kernel_manager_cluster().acpi_manager.lock().unwrap();
    if !acpi_manager.is_available() {
        pr_info!("ACPI is not available.");
        return true;
    }
    if !super::device::acpi::setup_interrupt(&acpi_manager) {
        pr_err!("Cannot setup ACPI interrupt.");
        return false;
    }
    if !acpi_manager.enable_acpi() {
        pr_err!("Cannot enable ACPI.");
        return false;
    }
    if !acpi_manager.enable_power_button(&mut get_kernel_manager_cluster().acpi_event_manager) {
        pr_err!("Cannot enable power button.");
        return false;
    }
    return true;
}

/// Init Timer
///
/// This function tries to set up LocalApicTimer.
/// If TSC-Deadline mode is usable, this will enable it and return.
/// Otherwise, this will calculate the frequency of the Local APIC Timer with ACPI PM Timer or
/// PIT.(ACPI PM Timer is prioritized.)
/// After that, this registers the timer to InterruptManager.
pub fn init_timer() -> LocalApicTimer {
    /* This function assumes that interrupt is not enabled */
    /* This function does not enable interrupt */
    let mut local_apic_timer = LocalApicTimer::new();
    local_apic_timer.init();
    if local_apic_timer.enable_deadline_mode(
        InterruptionIndex::LocalApicTimer as u16,
        get_cpu_manager_cluster()
            .interrupt_manager
            .lock()
            .unwrap()
            .get_local_apic_manager(),
    ) {
        pr_info!("Using Local APIC TSC Deadline Mode");
    } else if let Some(pm_timer) = get_kernel_manager_cluster()
        .acpi_manager
        .lock()
        .unwrap()
        .get_xsdt_manager()
        .get_fadt_manager()
        .get_acpi_pm_timer()
    {
        pr_info!("Using ACPI PM Timer to calculate frequency of Local APIC Timer.");
        local_apic_timer.set_up_interrupt(
            InterruptionIndex::LocalApicTimer as u16,
            get_cpu_manager_cluster()
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
        local_apic_timer.set_up_interrupt(
            InterruptionIndex::LocalApicTimer as u16,
            get_cpu_manager_cluster()
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

    get_cpu_manager_cluster()
        .interrupt_manager
        .lock()
        .unwrap()
        .set_ist(1, 0x4000.into());

    get_cpu_manager_cluster()
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

pub static AP_BOOT_COMPLETE_FLAG: AtomicBool = AtomicBool::new(false);

/// Allocate CpuManager and set self pointer
pub fn setup_cpu_manager_cluster(
    cpu_manager_address: Option<VAddress>,
) -> &'static mut CpuManagerCluster {
    let cpu_manager_address = cpu_manager_address.unwrap_or_else(|| {
        get_kernel_manager_cluster()
            .boot_strap_cpu_manager /* Allocate from BSP Object Manager */
            .object_allocator
            .lock()
            .unwrap()
            .alloc(
                core::mem::size_of::<CpuManagerCluster>().into(),
                &get_kernel_manager_cluster().memory_manager,
            )
            .unwrap()
    });
    let cpu_manager = unsafe { &mut *(cpu_manager_address.to_usize() as *mut CpuManagerCluster) };
    /*
        "mov rax, gs:0" is same as "let rax = *(gs as *const u64)".
        we cannot load gs.base by "lea rax, [gs:0]" because lea cannot use gs register in x86_64.
        On general kernel, the per-CPU's data struct has a member pointing itself and accesses it.
    */
    cpu_manager.arch_depend_data.self_pointer = cpu_manager_address.to_usize();
    unsafe {
        cpu::set_gs_and_kernel_gs_base(
            &cpu_manager.arch_depend_data.self_pointer as *const _ as u64,
        )
    };
    cpu_manager.list = PtrLinkedListNode::new();
    unsafe {
        cpu_manager
            .list
            .set_ptr_from_usize(cpu_manager_address.to_usize())
    };
    cpu_manager
}

/// Init APs
///
/// This function will setup multiple processors by using ACPI
/// This is in the development
pub fn init_multiple_processors_ap() {
    /* Get available Local APIC IDs from ACPI */
    let apic_id_list_iter = get_kernel_manager_cluster()
        .acpi_manager
        .lock()
        .unwrap()
        .get_xsdt_manager()
        .get_madt_manager()
        .find_apic_id_list();
    /* 0 ~ PAGE_SIZE is allocated as boot code TODO: allocate dynamically */
    let boot_address = 0usize;

    /* Set BSP Local APIC ID into cpu_manager */
    let mut cpu_manager = get_cpu_manager_cluster();
    let bsp_apic_id = get_cpu_manager_cluster()
        .interrupt_manager
        .lock()
        .unwrap()
        .get_local_apic_manager()
        .get_apic_id();
    cpu_manager.cpu_id = bsp_apic_id as usize;

    /* Extern Assembly Symbols */
    extern "C" {
        /* boot/boot_ap.s */
        fn ap_entry();
        fn ap_entry_end();
        static mut ap_os_stack_address: u64;
    }
    let ap_entry_address = ap_entry as *const fn() as usize;
    let ap_entry_end_address = ap_entry_end as *const fn() as usize;

    /* Copy boot code for application processors */
    let vector = ((boot_address >> PAGE_SHIFT) & 0xff) as u8;
    unsafe {
        core::ptr::copy_nonoverlapping(
            ap_entry_address as *const u8,
            boot_address as *mut u8,
            ap_entry_end_address - ap_entry_address,
        )
    };

    /* Allocate and set temporary stack */
    let stack_size = MSize::new(ContextManager::DEFAULT_STACK_SIZE_OF_SYSTEM);
    let stack = get_kernel_manager_cluster()
        .memory_manager
        .lock()
        .unwrap()
        .alloc_with_option(
            stack_size.to_order(None).to_page_order(),
            MemoryPermissionFlags::data(),
            MemoryOptionFlags::DIRECT_MAP,
        )
        .unwrap();
    unsafe {
        *(((&mut ap_os_stack_address as *mut _ as usize) - ap_entry_address + boot_address)
            as *mut u64) = (stack + stack_size).to_usize() as u64
    };

    let timer = get_kernel_manager_cluster()
        .acpi_manager
        .lock()
        .unwrap()
        .get_xsdt_manager()
        .get_fadt_manager()
        .get_acpi_pm_timer()
        .unwrap();

    let mut num_of_cpu = 1usize;
    'ap_init_loop: for apic_id in apic_id_list_iter {
        if apic_id == bsp_apic_id {
            continue;
        }
        num_of_cpu += 1;

        AP_BOOT_COMPLETE_FLAG.store(false, core::sync::atomic::Ordering::Relaxed);

        let interrupt_manager = get_kernel_manager_cluster()
            .boot_strap_cpu_manager
            .interrupt_manager
            .lock()
            .unwrap();
        interrupt_manager
            .get_local_apic_manager()
            .send_interrupt_command(apic_id, 0b101 /*INIT*/, 1, false, 0);

        timer.busy_wait_us(100);

        interrupt_manager
            .get_local_apic_manager()
            .send_interrupt_command(apic_id, 0b101 /*INIT*/, 1, true, 0);

        /* Wait 10 millisecond for the AP */
        timer.busy_wait_ms(10);

        interrupt_manager
            .get_local_apic_manager()
            .send_interrupt_command(apic_id, 0b110 /* Startup IPI*/, 0, false, vector);

        timer.busy_wait_us(200);

        interrupt_manager
            .get_local_apic_manager()
            .send_interrupt_command(apic_id, 0b110 /* Startup IPI*/, 0, false, vector);

        drop(interrupt_manager);
        for _wait in 0..5000
        /* Wait 5s for AP init */
        {
            if AP_BOOT_COMPLETE_FLAG.load(core::sync::atomic::Ordering::Relaxed) {
                continue 'ap_init_loop;
            }
            timer.busy_wait_ms(1);
        }
        panic!("Cannot init CPU(APIC ID: {})", apic_id);
    }

    /* Free boot_address */
    if let Err(e) = get_kernel_manager_cluster()
        .memory_manager
        .lock()
        .unwrap()
        .free(boot_address.into())
    {
        pr_err!("Cannot free boot_address: {:?}", e);
    }

    /* Free temporary stack */
    if let Err(e) = get_kernel_manager_cluster()
        .memory_manager
        .lock()
        .unwrap()
        .free(stack)
    {
        pr_err!("Cannot free temporary stack: {:?}", e);
    }

    if num_of_cpu != 1 {
        pr_info!("Found {} CPUs", num_of_cpu);
    }
}
