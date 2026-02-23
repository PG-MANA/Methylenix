//!
//! The arch-depended functions for initialization
//!
//! This module includes init codes for devices, memory, and task system.
//! This module is called by boot function.
//!

use crate::arch::target_arch::{
    context::{ContextManager, memory_layout::physical_address_to_direct_map},
    device::{cpu, jh7110_timer::Jh7110Timer},
    interrupt::{InterruptManager, plicv1::PlatformLevelInterruptController},
    paging::{PAGE_MASK, PAGE_SIZE_USIZE},
};

use crate::kernel::{
    collections::{init_struct, ptr_linked_list::PtrLinkedListNode},
    drivers::{
        boot_information::BootInformation,
        dtb::DtbManager,
        efi::{EFI_DTB_TABLE_GUID, EFI_PAGE_SIZE, memory_map::EfiMemoryType},
    },
    file_manager::elf::ELF_PROGRAM_HEADER_SEGMENT_LOAD,
    initialization::{idle, init_task_ap, init_work_queue},
    manager_cluster::{CpuManagerCluster, get_cpu_manager_cluster, get_kernel_manager_cluster},
    memory_manager::{
        MemoryManager,
        data_type::{Address, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress, VAddress},
        memory_allocator::MemoryAllocator,
        physical_memory_manager::PhysicalMemoryManager,
        system_memory_manager::{SystemMemoryManager, get_physical_memory_manager},
        virtual_memory_manager::VirtualMemoryManager,
    },
    task_manager::{TaskManager, run_queue::RunQueue},
    timer_manager::LocalTimerManager,
};

use core::sync::atomic::AtomicBool;

/// Memory Areas for PhysicalMemoryManager
static mut MEMORY_FOR_PHYSICAL_MEMORY_MANAGER: [u8; PAGE_SIZE_USIZE * 2] = [0; PAGE_SIZE_USIZE * 2];

pub static AP_BOOT_COMPLETE_FLAG: AtomicBool = AtomicBool::new(false);

/// Setup Per CPU struct
///
/// This function must be called on the cpu that is going to own returned manager.
pub fn setup_cpu_manager_cluster(
    cpu_manager_address: Option<VAddress>,
    mhartid: u64,
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

    unsafe { cpu::set_cpu_base_address(cpu_manager as *const _ as u64) };
    init_struct!(cpu_manager.list, PtrLinkedListNode::new());
    unsafe {
        get_kernel_manager_cluster()
            .cpu_list
            .insert_tail(&mut cpu_manager.list)
    };
    cpu_manager.cpu_id = mhartid as usize;
    cpu_manager.arch_depend_data.mhartid = mhartid;
    cpu_manager
}

/// Init memory system based on boot information.
/// This function sets up PhysicalMemoryManager which manages where is free
/// and [`VirtualMemoryManager`] which manages which process is using what area of virtual memory.
/// After that, this will set up MemoryManager.
/// If one of the processes is failed, this will panic.
pub fn init_memory_by_boot_information(boot_information: &mut BootInformation) {
    /* Set up Physical Memory Manager */
    let mut physical_memory_manager = PhysicalMemoryManager::new();
    unsafe {
        physical_memory_manager.add_memory_entry_pool(
            &raw const MEMORY_FOR_PHYSICAL_MEMORY_MANAGER as usize,
            size_of_val(
                (&raw const MEMORY_FOR_PHYSICAL_MEMORY_MANAGER)
                    .as_ref()
                    .unwrap(),
            ),
        );
    }

    let mut max_usable_memory_address = PAddress::new(0);

    /* Free usable memory area */
    for entry in boot_information.memory_map.iter() {
        match entry.memory_type {
            EfiMemoryType::EfiMaxMemoryType => continue,
            EfiMemoryType::EfiConventionalMemory
            | EfiMemoryType::EfiBootServicesCode
            | EfiMemoryType::EfiLoaderCode => {
                let start_address = PAddress::new(entry.physical_start);
                let size = MSize::new((entry.number_of_pages as usize) * EFI_PAGE_SIZE);
                bug_on_err!(physical_memory_manager.free(start_address, size, true));
                if start_address + size > max_usable_memory_address {
                    max_usable_memory_address = start_address + size;
                }
            }
            _ => {}
        }
        pr_info!(
            "[{:#016X}~{:#016X}] {}",
            entry.physical_start,
            MSize::new((entry.number_of_pages as usize) << 12)
                .to_end_address(PAddress::new(entry.physical_start))
                .to_usize(),
            entry.memory_type
        );
    }

    /* Set up Virtual Memory Manager */
    let mut virtual_memory_manager = VirtualMemoryManager::new();
    virtual_memory_manager.init_system(&mut physical_memory_manager);
    init_struct!(
        get_kernel_manager_cluster().system_memory_manager,
        SystemMemoryManager::new(physical_memory_manager)
    );
    get_kernel_manager_cluster()
        .system_memory_manager
        .init_pools(&mut virtual_memory_manager);

    for entry in boot_information.elf_program_headers.iter() {
        let virtual_address = entry.get_virtual_address() as usize;
        let physical_address = entry.get_physical_address() as usize;
        if entry.get_segment_type() != ELF_PROGRAM_HEADER_SEGMENT_LOAD
            || (virtual_address & !PAGE_MASK) != 0
            || (physical_address & !PAGE_MASK) != 0
            || entry.get_memory_size() == 0
        {
            continue;
        }
        let aligned_size = MemoryManager::size_align(MSize::new(entry.get_memory_size() as usize));
        let permission = MemoryPermissionFlags::new(
            entry.is_segment_readable(),
            entry.is_segment_writable(),
            entry.is_segment_executable(),
            false,
        );
        match virtual_memory_manager.map_address(
            PAddress::new(physical_address),
            Some(VAddress::new(virtual_address)),
            aligned_size,
            permission,
            MemoryOptionFlags::KERNEL,
            get_physical_memory_manager(),
        ) {
            Ok(address) => {
                assert_eq!(
                    address,
                    VAddress::new(virtual_address),
                    "Virtual Address is different from Physical Address: V:{:#X} P:{:#X}",
                    address.to_usize(),
                    virtual_address
                );
            }
            Err(e) => {
                panic!("Mapping ELF Section was failed: {:?}", e);
            }
        };
        pr_info!(
            "VA: [{:#016X}~{:#016X}] => PA: [{:#016X}~{:#016X}] (R: {}, W: {}, E: {})",
            virtual_address,
            virtual_address + aligned_size.to_usize(),
            physical_address,
            physical_address + aligned_size.to_usize(),
            permission.is_readable(),
            permission.is_writable(),
            permission.is_executable()
        );
    }

    /* Set up Memory Manager */
    init_struct!(
        get_kernel_manager_cluster().kernel_memory_manager,
        MemoryManager::new(virtual_memory_manager)
    );

    /* Adjust Memory Pointer */
    /* `efi_memory_map_address` and `elf_program_headers_address` are already direct mapped. */
    if let Some(system_table) = &mut boot_information.efi_system_table {
        system_table.set_configuration_table(
            physical_address_to_direct_map(PAddress::new(system_table.get_configuration_table()))
                .to_usize(),
        );
    }
    /*boot_information.font_address = boot_information.font_address.map(|a| {
        (
            (physical_address_to_direct_map(PAddress::new(a.0)).to_usize()),
            a.1,
        )
    });*/

    /* Apply paging */
    get_kernel_manager_cluster()
        .kernel_memory_manager
        .set_paging_table();

    /* Set up Kernel Memory Allocator */
    let mut memory_allocator = MemoryAllocator::new();
    memory_allocator
        .init()
        .expect("Failed to init MemoryAllocator");
    init_struct!(get_cpu_manager_cluster().memory_allocator, memory_allocator);

    /* TODO: free EfiLoaderData area excepting kernel area */
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
            if dtb_manager.is_node_operational(&n)
                || !dtb_manager.is_device_compatible(&n, b"riscv,plic0")
            {
                node = Some(n);
                continue;
            }
            if let Ok(mut controller) = PlatformLevelInterruptController::new(PAddress::new(
                dtb_manager.read_reg_property(&n, 0).unwrap().0,
            )) {
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
pub fn init_dtb(boot_information: &BootInformation, mut dtb_address: Option<usize>) -> bool {
    let mut dtb_manager = DtbManager::new();
    if dtb_address.is_none()
        && let Some(system_table) = &boot_information.efi_system_table
    {
        for e in unsafe { system_table.get_configuration_table_slice() } {
            if e.vendor_guid == EFI_DTB_TABLE_GUID {
                dtb_address = Some(e.vendor_table);
                break;
            }
        }
    }
    if dtb_address.is_none() {
        init_struct!(
            get_kernel_manager_cluster().arch_depend_data.dtb_manager,
            dtb_manager
        );
        return false;
    }

    if !dtb_manager.init(PAddress::new(dtb_address.unwrap())) {
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

pub fn init_local_timer(_acpi_available: bool, dtb_available: bool) {
    init_struct!(
        get_cpu_manager_cluster().local_timer_manager,
        LocalTimerManager::new()
    );
    /* TODO: support various controller */
    init_struct!(
        get_cpu_manager_cluster().arch_depend_data.jh7110_timer,
        Jh7110Timer::new()
    );

    let jh7110_timer = &mut get_cpu_manager_cluster().arch_depend_data.jh7110_timer;
    // TODO: Implement Timet trait to jh7110_timer
    // let local_timer_manager = &mut get_cpu_manager_cluster().local_timer_manager;

    if dtb_available {
        let dtb_manager = &get_kernel_manager_cluster().arch_depend_data.dtb_manager;
        let mut node = None;

        while let Some(info) = dtb_manager.search_node(b"timer", node.as_ref()) {
            if dtb_manager.is_node_operational(&info) {
                if jh7110_timer.init_with_dtb(dtb_manager, &info) {
                    // local_timer_manager.set_source_timer(jh7110_timer);
                    return;
                }
            }
            node = Some(info);
        }
    }
    panic!("Failed to initialize the local timer");
}

fn init_local_timer_ap() {
    init_struct!(
        get_cpu_manager_cluster().local_timer_manager,
        LocalTimerManager::new()
    );
    /* TODO: support various controller */
    init_struct!(
        get_cpu_manager_cluster().arch_depend_data.jh7110_timer,
        Jh7110Timer::new()
    );

    get_cpu_manager_cluster()
        .arch_depend_data
        .jh7110_timer
        .init_ap(
            &get_kernel_manager_cluster()
                .boot_strap_cpu_manager
                .arch_depend_data
                .jh7110_timer,
        );

    /*get_cpu_manager_cluster()
    .local_timer_manager
    .set_source_timer(&get_cpu_manager_cluster().arch_depend_data.jh7110_timer);*/
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

pub fn wake_up_application_processors(_acpi_available: bool, _dtb_available: bool) {
    pr_info!("TODO: Wake up application processors...");
}

pub extern "C" fn ap_boot_main(mhartid: u64) -> ! {
    /* Setup CPU Manager, it contains individual data of CPU */
    let cpu_manager = setup_cpu_manager_cluster(None, mhartid);
    pr_info!("Booted (mhartid: {:#X})", mhartid);

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
        .jh7110_timer
        .start_interrupt();
    idle()
}
