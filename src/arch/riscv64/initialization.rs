//!
//! The arch-depended functions for initialization
//!
//! This module includes init codes for devices, memory, and task system.
//! This module is called by boot function.
//!

use crate::arch::target_arch::{
    context::ContextManager,
    device::{cpu, jh7110_timer::Jh7110Timer, sbi},
    get_hartid,
    interrupt::{InterruptManager, plicv1::PlatformLevelInterruptController},
    paging::{PAGE_SIZE, PAGE_SIZE_USIZE},
};

use crate::kernel::{
    collections::{init_struct, ptr_linked_list::PtrLinkedListNode},
    drivers::{
        acpi::table::madt::MadtManager, boot_information::BootInformation, dtb::DtbManager,
        efi::EFI_DTB_TABLE_GUID,
    },
    initialization::{idle, init_task_ap, init_work_queue},
    manager_cluster::{CpuManagerCluster, get_cpu_manager_cluster, get_kernel_manager_cluster},
    memory_manager::{
        alloc_pages, alloc_pages_with_physical_address,
        data_type::{Address, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress, VAddress},
        free_pages,
        memory_allocator::MemoryAllocator,
    },
    task_manager::{TaskManager, run_queue::RunQueue},
    timer_manager::LocalTimerManager,
};

use core::sync::atomic::AtomicBool;

use alloc::boxed::Box;

pub static AP_BOOT_COMPLETE_FLAG: AtomicBool = AtomicBool::new(false);

/// Setup Per CPU struct
///
/// This function must be called on the cpu that is going to own returned manager.
pub fn setup_cpu_manager_cluster(
    cpu_manager_address: Option<VAddress>,
    hartid: u64,
) -> &'static mut CpuManagerCluster<'static> {
    let cpu_manager_address = cpu_manager_address.unwrap_or_else(|| {
        /* ATTENTION: BSP must be sleeping. */
        get_kernel_manager_cluster()
            .boot_strap_cpu_manager /* Allocate from BSP Object Manager */
            .memory_allocator
            .kmalloc(MSize::new(size_of::<CpuManagerCluster>()))
            .expect("Failed to alloc CpuManagerCluster")
    });
    let cpu_manager = unsafe { &mut *(cpu_manager_address.to::<CpuManagerCluster>()) };
    /* Initialize some essential members */
    init_struct!(cpu_manager.memory_allocator, MemoryAllocator::new());
    init_struct!(cpu_manager.run_queue, RunQueue::new());
    init_struct!(cpu_manager.interrupt_manager, InterruptManager::new());
    init_struct!(cpu_manager.list, PtrLinkedListNode::new());

    unsafe { cpu::set_cpu_base_address(cpu_manager as *const _ as u64) };
    unsafe {
        get_kernel_manager_cluster()
            .cpu_list
            .insert_tail(&mut cpu_manager.list)
    };
    cpu_manager.cpu_id = hartid as usize;
    cpu_manager.arch_depend_data.hartid = hartid;
    cpu_manager
}

/// Init InterruptManager
pub fn init_interrupt(acpi_available: bool, dtb_available: bool) {
    init_struct!(
        get_cpu_manager_cluster().interrupt_manager,
        InterruptManager::new()
    );

    get_cpu_manager_cluster().interrupt_manager.init();

    if acpi_available {
        pr_warn!("ACPI is not supported");
    }

    if dtb_available {
        /* TODO: support various controller */
        let dtb_manager = &get_kernel_manager_cluster().arch_depend_data.dtb_manager;
        let mut node = None;
        while let Some(n) = dtb_manager.search_node(b"plic", node.as_ref()) {
            if !dtb_manager.is_node_operational(&n)
                || !dtb_manager.is_device_compatible(&n, b"riscv,plic0")
            {
                node = Some(n);
                continue;
            }
            let (address, size) = dtb_manager.read_reg_property(&n, 0).unwrap();
            let Some(ndev) = dtb_manager
                .get_property(&n, b"riscv,ndev")
                .and_then(|p| dtb_manager.read_property_as_u32(&p, 0))
            else {
                pr_warn!("The number of devices is not available.");
                node = Some(n);
                continue;
            };
            if let Ok(mut controller) = PlatformLevelInterruptController::new(
                PAddress::new(address),
                MSize::new(size),
                ndev,
            ) {
                assert!(controller.init(), "Failed to initialize PLIC");
                init_struct!(
                    get_kernel_manager_cluster().arch_depend_data.plic,
                    controller
                );
                get_cpu_manager_cluster().interrupt_manager.init_ipi();
                return;
            }
            node = Some(n);
        }
    }

    panic!("Failed to initialize interrupt controller");
}

pub fn init_interrupt_ap(cpu_manager_cluster: &mut CpuManagerCluster) {
    let mut interrupt_manager = InterruptManager::new();
    interrupt_manager.init_ap();
    interrupt_manager.init_ipi();

    init_struct!(cpu_manager_cluster.interrupt_manager, interrupt_manager);
}

/// Init SerialPort
///
/// This function does not enable the interrupt.
pub fn init_serial_port(acpi_available: bool, dtb_available: bool) -> bool {
    (acpi_available
        && get_kernel_manager_cluster()
            .serial_port_manager
            .init_with_acpi())
        || (dtb_available
            && get_kernel_manager_cluster()
                .serial_port_manager
                .init_with_dtb())
}

/// Init Device Tree Blob Manager
pub fn init_dtb(boot_information: &BootInformation) -> bool {
    let mut dtb_manager = DtbManager::new();
    let Some(dtb_address) = boot_information.dtb_address.map(|a| a.get()).or_else(|| {
        boot_information
            .efi_system_table
            .as_ref()
            .and_then(|t| t.find_vendor_table(EFI_DTB_TABLE_GUID))
    }) else {
        init_struct!(
            get_kernel_manager_cluster().arch_depend_data.dtb_manager,
            dtb_manager
        );
        return false;
    };

    if !dtb_manager.init(PAddress::new(dtb_address)) {
        pr_warn!("Failed to initialize DTB.");
        init_struct!(
            get_kernel_manager_cluster().arch_depend_data.dtb_manager,
            dtb_manager
        );
        return false;
    }
    init_struct!(
        get_kernel_manager_cluster().arch_depend_data.dtb_manager,
        dtb_manager
    );
    true
}

fn init_jh7110_timer(_acpi_available: bool, dtb_available: bool) -> bool {
    let mut jh7110_timer = Jh7110Timer::new();
    if dtb_available {
        let dtb_manager = &get_kernel_manager_cluster().arch_depend_data.dtb_manager;
        let mut node = None;

        while let Some(info) = dtb_manager.search_node(b"timer", node.as_ref()) {
            if dtb_manager.is_node_operational(&info)
                && jh7110_timer.init_with_dtb(dtb_manager, &info)
            {
                init_struct!(
                    get_cpu_manager_cluster().arch_depend_data.timer,
                    Box::new(jh7110_timer)
                );
                return true;
            }
            node = Some(info);
        }
    }
    false
}

fn init_sbi_timer(_acpi_available: bool, dtb_available: bool) -> bool {
    if dtb_available {
        let dtb_manager = &get_kernel_manager_cluster().arch_depend_data.dtb_manager;
        if let Some(frequency) = dtb_manager
            .search_node(b"cpus", None)
            .and_then(|node| dtb_manager.get_property(&node, b"timebase-frequency"))
            .and_then(|p| dtb_manager.read_property_as_u32(&p, 0))
        {
            pr_info!("CPU's timer frequency: {frequency:#X}");
            return match sbi::SbiTimer::new(frequency as u64) {
                Ok(t) => {
                    let t = Box::new(t);
                    /*get_cpu_manager_cluster()
                    .local_timer_manager
                    .set_source_timer(t.as_ref());*/
                    init_struct!(get_cpu_manager_cluster().arch_depend_data.timer, t);
                    true
                }
                Err(e) => {
                    pr_info!("SBI Timer Extension is not supported: {e:#?}");
                    false
                }
            };
        } else {
            pr_warn!("Failed to get CPU's timer frequency")
        }
    }
    false
}

pub fn init_local_timer(acpi_available: bool, dtb_available: bool) {
    init_struct!(
        get_cpu_manager_cluster().local_timer_manager,
        LocalTimerManager::new()
    );
    /* TODO: support various controller */
    if init_jh7110_timer(acpi_available, dtb_available) {
        return;
    }
    if init_sbi_timer(acpi_available, dtb_available) {
        return;
    }
    panic!("Failed to initialize the local timer");
}

fn init_local_timer_ap() {
    let acpi_available = get_kernel_manager_cluster()
        .acpi_manager
        .lock()
        .unwrap()
        .is_available();
    let dtb_available = get_kernel_manager_cluster()
        .arch_depend_data
        .dtb_manager
        .is_available();
    init_local_timer(acpi_available, dtb_available);
}

/// Init TaskManager
pub fn init_task(main_process: fn() -> !, idle_process: fn() -> !) {
    let mut context_manager = ContextManager::new();
    let mut run_queue = RunQueue::new();
    let mut task_manager = TaskManager::new();

    context_manager.init();

    run_queue.init().expect("Failed to init RunQueue");

    let main_context = context_manager
        .create_system_context(main_process, None)
        .expect("Cannot create main thread's context.");
    let idle_context = context_manager
        .create_system_context(idle_process, Some(ContextManager::IDLE_THREAD_STACK_SIZE))
        .expect("Cannot create idle thread's context.");

    task_manager.init(context_manager, main_context, idle_context, &mut run_queue);

    init_struct!(get_cpu_manager_cluster().run_queue, run_queue);
    init_struct!(get_kernel_manager_cluster().task_manager, task_manager);
}

pub fn wake_up_application_processors(acpi_available: bool, dtb_available: bool) {
    /* For ACPI */
    let acpi_madt_manager;
    /* For Devicetree */
    let mut dtb_cpu_node = None;

    /* Prepare the structs needed by `mpidr_iter` */
    if acpi_available {
        acpi_madt_manager = get_kernel_manager_cluster()
            .acpi_manager
            .lock()
            .unwrap()
            .get_table_manager()
            .get_table_manager::<MadtManager>();
        if acpi_madt_manager.is_none() {
            pr_info!("ACPI does not have MADT.");
            return;
        };
        unimplemented!();
    } else if dtb_available {
        /* Do nothing */
    } else {
        pr_info!("Failed to get processors information");
        return;
    }

    let mut hartid_iter = || {
        if acpi_available {
            unimplemented!()
        } else if dtb_available {
            let dtb_manager = &get_kernel_manager_cluster().arch_depend_data.dtb_manager;
            if let Some(n) = dtb_manager.search_node(b"cpu", dtb_cpu_node.as_ref()) {
                let hartid = dtb_manager
                    .read_reg_property(&n, 0)
                    .map(|(hartid, _)| hartid as u64);
                dtb_cpu_node = Some(n);
                hartid
            } else {
                None
            }
        } else {
            unreachable!();
        }
    };

    /* Extern Assembly Symbols */
    unsafe extern "C" {
        /* device/cpu.rs */
        fn ap_entry();
        fn ap_entry_end();
        fn ap_temporary_interrupt_vector();
    }
    let ap_entry_address = ap_entry as *const fn() as usize;
    let ap_entry_end_address = ap_entry_end as *const fn() as usize;
    let (virtual_address, physical_address) = alloc_pages_with_physical_address!(
        PAGE_SIZE.to_order(None).to_page_order(),
        MemoryPermissionFlags::data(),
        MemoryOptionFlags::KERNEL
    )
    .expect("Failed to allocate memory for AP");
    /* Copy boot code for application processors */
    assert!(
        (ap_entry_end_address - ap_entry_address) <= PAGE_SIZE_USIZE,
        "The size of ap_entry:{:#X}",
        (ap_entry_end_address - ap_entry_address)
    );

    unsafe {
        core::ptr::copy_nonoverlapping(
            ap_entry as *const u8,
            virtual_address.to_usize() as *mut u8,
            ap_entry_end_address - ap_entry_address,
        )
    };

    /* Allocate and set temporary stack */
    let stack_size = MSize::new(ContextManager::DEFAULT_STACK_SIZE_OF_SYSTEM);
    let stack = alloc_pages!(stack_size.to_order(None).to_page_order())
        .expect("Failed to alloc the initial stack for AP");
    let boot_data = [
        cpu::get_atp(),
        ap_temporary_interrupt_vector as *const fn() as usize as u64,
        (stack + stack_size).to_usize() as u64,
        ap_boot_main as *const fn() as usize as u64,
    ];
    assert!(
        MSize::new((ap_entry_end_address - ap_entry_address) + size_of_val(&boot_data)) < PAGE_SIZE
    );
    unsafe {
        *((virtual_address.to_usize() + (ap_entry_end_address - ap_entry_address)) as *mut _) =
            boot_data
    };
    cpu::flush_data_cache_all();

    let bsp_hartid = get_hartid();
    let mut num_of_cpu = 1usize;

    'ap_init_loop: while let Some(hartid) = hartid_iter() {
        if hartid == bsp_hartid {
            continue;
        }
        pr_info!("Boot the CPU (HartID: {hartid:#X})");
        AP_BOOT_COMPLETE_FLAG.store(false, core::sync::atomic::Ordering::Relaxed);
        cpu::synchronize(AP_BOOT_COMPLETE_FLAG.as_ptr());
        if let Err(e) = sbi::hart_start(hartid as _, physical_address.to_usize(), 0) {
            pr_err!("Failed to startup the CPU: {e:?}");
            continue;
        }
        loop {
            cpu::synchronize(AP_BOOT_COMPLETE_FLAG.as_ptr());
            if AP_BOOT_COMPLETE_FLAG.load(core::sync::atomic::Ordering::Relaxed) {
                num_of_cpu += 1;
                continue 'ap_init_loop;
            }
            core::hint::spin_loop();
        }
    }

    let _ = free_pages!(virtual_address);
    let _ = free_pages!(stack);

    if num_of_cpu != 1 {
        pr_info!("Found {} CPUs", num_of_cpu);
    }
}

pub extern "C" fn ap_boot_main(hartid: u64) -> ! {
    /* Setup CPU Manager, it contains individual data of CPU */
    let cpu_manager = setup_cpu_manager_cluster(None, hartid);
    pr_info!("Booted (hartid: {:#X})", hartid);

    /* Set up the memory management system */
    let mut memory_allocator = MemoryAllocator::new();
    memory_allocator
        .init()
        .expect("Failed to init MemoryAllocator");
    init_struct!(cpu_manager.memory_allocator, memory_allocator);

    /* Set up the interrupt */
    init_interrupt_ap(cpu_manager);
    init_local_timer_ap();

    /* Set up the task management system */
    init_task_ap(ap_idle);
    init_work_queue();

    /* Switch to ap_idle task with own stack */
    cpu_manager.run_queue.start()
}

fn ap_idle() -> ! {
    AP_BOOT_COMPLETE_FLAG.store(true, core::sync::atomic::Ordering::Relaxed);
    get_cpu_manager_cluster()
        .arch_depend_data
        .timer
        .start_interrupt();
    idle()
}
