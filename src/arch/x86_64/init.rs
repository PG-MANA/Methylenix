//!
//! Init codes
//!
//! This module including init codes for device, memory, and task system.
//! This module is called by boot function.

pub mod multiboot;

use crate::arch::target_arch::context::memory_layout::physical_address_to_direct_map;
use crate::arch::target_arch::context::ContextManager;
use crate::arch::target_arch::device::io_apic::IoApicManager;
use crate::arch::target_arch::device::local_apic_timer::LocalApicTimer;
use crate::arch::target_arch::device::pci::ArchDependPciManager;
use crate::arch::target_arch::device::pit::PitManager;
use crate::arch::target_arch::device::{cpu, pic};
use crate::arch::target_arch::interrupt::{InterruptIndex, InterruptManager};
use crate::arch::target_arch::paging::{PAGE_SHIFT, PAGE_SIZE, PAGE_SIZE_USIZE};

use crate::kernel::collections::ptr_linked_list::PtrLinkedListNode;
use crate::kernel::drivers::acpi::device::AcpiDeviceManager;
use crate::kernel::drivers::acpi::table::{madt::MadtManager, mcfg::McfgManager};
use crate::kernel::drivers::acpi::AcpiManager;
use crate::kernel::drivers::pci::PciManager;
use crate::kernel::manager_cluster::{
    get_cpu_manager_cluster, get_kernel_manager_cluster, CpuManagerCluster,
};
use crate::kernel::memory_manager::data_type::{
    Address, MSize, MemoryPermissionFlags, PAddress, VAddress,
};
use crate::kernel::sync::spin_lock::Mutex;
use crate::kernel::task_manager::run_queue::RunQueue;
use crate::kernel::task_manager::TaskManager;
use crate::kernel::timer_manager::{GlobalTimerManager, LocalTimerManager, Timer};

use core::mem;
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
    idle_process: fn() -> !,
) {
    let mut context_manager = ContextManager::new();
    let mut run_queue = RunQueue::new();
    let mut task_manager = TaskManager::new();

    context_manager.init(
        system_cs,
        0, /*is it ok?*/
        user_cs,
        user_ss,
        unsafe { cpu::get_cr3() },
    );

    run_queue.init().expect("Failed to init RunQueue");

    let main_context = context_manager
        .create_system_context(main_process, None)
        .expect("Cannot create main thread's context.");
    let idle_context = context_manager
        .create_system_context(idle_process, Some(ContextManager::IDLE_THREAD_STACK_SIZE))
        .expect("Cannot create idle thread's context.");

    task_manager.init(context_manager, main_context, idle_context, &mut run_queue);

    mem::forget(mem::replace(
        &mut get_cpu_manager_cluster().run_queue,
        run_queue,
    ));
    mem::forget(mem::replace(
        &mut get_kernel_manager_cluster().task_manager,
        task_manager,
    ));
}

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
        .init(&mut get_kernel_manager_cluster().task_manager);
}

/// Init InterruptManager
///
/// This function disables 8259 PIC and init InterruptManager
pub fn init_interrupt(kernel_selector: u16) {
    pic::disable_8259_pic();

    get_cpu_manager_cluster().interrupt_manager = InterruptManager::new();
    get_cpu_manager_cluster()
        .interrupt_manager
        .init(kernel_selector);
    let mut io_apic_manager = IoApicManager::new();
    io_apic_manager.init();
    mem::forget(mem::replace(
        &mut get_kernel_manager_cluster()
            .arch_depend_data
            .io_apic_manager,
        Mutex::new(io_apic_manager),
    ));
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
        mem::forget(mem::replace(
            &mut get_kernel_manager_cluster().acpi_manager,
            Mutex::new(a),
        ));
        mem::forget(mem::replace(
            &mut get_kernel_manager_cluster().acpi_device_manager,
            d,
        ));
    };

    if !acpi_manager.init(rsdp_ptr, &mut device_manager) {
        pr_warn!("Cannot init ACPI.");
        set_manger(acpi_manager, device_manager);
        return false;
    }
    if let Some(e) = acpi_manager.create_acpi_event_manager() {
        mem::forget(mem::replace(
            &mut get_kernel_manager_cluster().acpi_event_manager,
            e,
        ));
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
    mem::forget(mem::replace(
        &mut get_kernel_manager_cluster().pci_manager,
        pci_manager,
    ));
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

/// Init Timer
///
/// This function tries to set up LocalApicTimer.
/// If TSC-Deadline mode is usable, this will enable it and return.
/// Otherwise, this will calculate the frequency of the Local APIC Timer with ACPI PM Timer or
/// PIT.(ACPI PM Timer is prioritized.)
/// After that, this registers the timer to InterruptManager.
pub fn init_local_timer() {
    /* This function assumes that interrupt is not enabled */
    /* This function does not enable interrupt */
    mem::forget(mem::replace(
        &mut get_cpu_manager_cluster().local_timer_manager,
        LocalTimerManager::new(),
    ));
    mem::forget(mem::replace(
        &mut get_cpu_manager_cluster().arch_depend_data.local_apic_timer,
        LocalApicTimer::new(),
    ));
    let local_apic_timer = &mut get_cpu_manager_cluster().arch_depend_data.local_apic_timer;
    let local_timer_manager = &mut get_cpu_manager_cluster().local_timer_manager;
    local_apic_timer.init();
    if local_apic_timer.enable_deadline_mode(
        InterruptIndex::LocalApicTimer as u16,
        get_cpu_manager_cluster()
            .interrupt_manager
            .get_local_apic_manager(),
    ) {
        pr_info!("Using Local APIC TSC Deadline Mode");
        local_timer_manager.set_source_timer(local_apic_timer);
    } else if let Some(pm_timer) = get_kernel_manager_cluster()
        .acpi_device_manager
        .get_pm_timer()
    {
        pr_info!("Using ACPI PM Timer to calculate frequency of Local APIC Timer.");
        local_apic_timer.set_up_interrupt(
            InterruptIndex::LocalApicTimer as u16,
            get_cpu_manager_cluster()
                .interrupt_manager
                .get_local_apic_manager(),
            pm_timer,
        );
        local_timer_manager.set_source_timer(local_apic_timer); /* Temporary, set local APIC Timer */
    } else {
        pr_info!("Using PIT to calculate frequency of Local APIC Timer.");
        let mut pit = PitManager::new();
        pit.init();
        local_apic_timer.set_up_interrupt(
            InterruptIndex::LocalApicTimer as u16,
            get_cpu_manager_cluster()
                .interrupt_manager
                .get_local_apic_manager(),
            &pit,
        );
        pit.stop_counting();
        local_timer_manager.set_source_timer(local_apic_timer); /* Temporary, set local APIC Timer */
    }

    get_cpu_manager_cluster()
        .interrupt_manager
        .set_device_interrupt_function(
            LocalApicTimer::local_apic_timer_handler,
            None,
            Some(InterruptIndex::LocalApicTimer as _),
            0,
            false,
        )
        .expect("Failed to setup the interrupt for Local APIC Timer");

    /* Setup TimerManager */
}

pub fn init_global_timer() {
    mem::forget(mem::replace(
        &mut get_kernel_manager_cluster().global_timer_manager,
        GlobalTimerManager::new(),
    ));
}

pub static AP_BOOT_COMPLETE_FLAG: AtomicBool = AtomicBool::new(false);

/// Allocate CpuManager and set self pointer
pub fn setup_cpu_manager_cluster(
    cpu_manager_address: Option<VAddress>,
) -> &'static mut CpuManagerCluster {
    let cpu_manager_address = cpu_manager_address.unwrap_or_else(|| {
        /* ATTENTION: BSP must be sleeping. */
        get_kernel_manager_cluster()
            .boot_strap_cpu_manager /* Allocate from BSP Object Manager */
            .memory_allocator
            .kmalloc(MSize::new(core::mem::size_of::<CpuManagerCluster>()))
            .expect("Failed to alloc CpuManagerCluster")
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
    mem::forget(mem::replace(
        &mut cpu_manager.list,
        PtrLinkedListNode::new(),
    ));
    get_kernel_manager_cluster()
        .cpu_list
        .insert_tail(&mut cpu_manager.list);
    cpu_manager
}

/// Init APs
///
/// This function will setup multiple processors by using ACPI
/// This is in the development
pub fn init_multiple_processors_ap() {
    /* 0 ~ PAGE_SIZE is allocated as boot code TODO: allocate dynamically */
    let boot_address = PAddress::new(0);

    /* Get available Local APIC IDs from ACPI */
    let madt_manager = get_kernel_manager_cluster()
        .acpi_manager
        .lock()
        .unwrap()
        .get_table_manager()
        .get_table_manager::<MadtManager>();
    if madt_manager.is_none() {
        pr_info!("ACPI does not have MADT.");
        return;
    }
    let madt_manager = madt_manager.unwrap();
    let apic_id_list_iter = madt_manager.find_apic_id_list();

    /* Set BSP Local APIC ID into cpu_manager */
    let mut cpu_manager = get_cpu_manager_cluster();
    let bsp_apic_id = get_cpu_manager_cluster()
        .interrupt_manager
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
    let vector = ((boot_address.to_usize() >> PAGE_SHIFT) & 0xff) as u8;
    assert!(ap_entry_end_address - ap_entry_address <= PAGE_SIZE_USIZE);
    unsafe {
        core::ptr::copy_nonoverlapping(
            ap_entry_address as *const u8,
            //physical_address_to_direct_map(PAddress::new(boot_address)).to_usize() as *mut u8,
            physical_address_to_direct_map(boot_address).to_usize() as *mut u8,
            ap_entry_end_address - ap_entry_address,
        )
    };

    /* Allocate and set temporary stack */
    let stack_size = MSize::new(ContextManager::DEFAULT_STACK_SIZE_OF_SYSTEM);
    let stack = get_kernel_manager_cluster()
        .kernel_memory_manager
        .alloc_pages_with_physical_address(
            stack_size.to_order(None).to_page_order(),
            MemoryPermissionFlags::data(),
            None,
        )
        .expect("Failed to alloc stack for AP");
    unsafe {
        *(physical_address_to_direct_map(PAddress::new(
            (&mut ap_os_stack_address as *mut _ as usize) - ap_entry_address
                + boot_address.to_usize(),
        ))
        .to_usize() as *mut u64) =
            physical_address_to_direct_map(stack.1 + stack_size).to_usize() as u64
    };

    let timer = get_kernel_manager_cluster()
        .acpi_device_manager
        .get_pm_timer()
        .expect("This computer has no ACPI PM Timer.")
        .clone();

    let mut num_of_cpu = 1usize;
    'ap_init_loop: for apic_id in apic_id_list_iter {
        if apic_id == bsp_apic_id {
            continue;
        }
        num_of_cpu += 1;

        AP_BOOT_COMPLETE_FLAG.store(false, core::sync::atomic::Ordering::Relaxed);

        let local_apic_manager = &get_kernel_manager_cluster()
            .boot_strap_cpu_manager
            .interrupt_manager
            .get_local_apic_manager();

        local_apic_manager.send_interrupt_command(apic_id, 0b101 /*INIT*/, 1, false, 0);

        timer.busy_wait_us(100);

        local_apic_manager.send_interrupt_command(apic_id, 0b101 /*INIT*/, 1, true, 0);

        /* Wait 10 millisecond for the AP */
        timer.busy_wait_ms(10);

        local_apic_manager
            .send_interrupt_command(apic_id, 0b110 /* Startup IPI*/, 0, false, vector);

        timer.busy_wait_us(200);

        local_apic_manager
            .send_interrupt_command(apic_id, 0b110 /* Startup IPI*/, 0, false, vector);

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
        .kernel_memory_manager
        .free_physical_memory(boot_address, PAGE_SIZE)
    {
        pr_err!("Cannot free boot_address: {:?}", e);
    }

    /* Free temporary stack */
    if let Err(e) = get_kernel_manager_cluster()
        .kernel_memory_manager
        .free(stack.0)
    {
        pr_err!("Cannot free temporary stack: {:?}", e);
    }

    madt_manager.release_memory_map();

    if num_of_cpu != 1 {
        pr_info!("Found {} CPUs", num_of_cpu);
    }
}
