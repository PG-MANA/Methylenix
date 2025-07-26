//!
//! The arch-depended functions for initialization
//!
//! This module includes init codes for devices, memory, and task system.
//! This module is called by boot function.
//!

use crate::arch::target_arch::{
    boot_info::BootInformation,
    context::{ContextManager, memory_layout::physical_address_to_direct_map},
    device::{
        cpu,
        generic_timer::{GenericTimer, SystemCounter},
    },
    interrupt::{InterruptManager, gic::GicDistributor, gic::read_interrupt_info_from_dtb},
    paging::{PAGE_MASK, PAGE_SIZE, PAGE_SIZE_USIZE},
};

use crate::kernel::{
    collections::{init_struct, ptr_linked_list::PtrLinkedListNode},
    drivers::{
        acpi::{
            AcpiManager,
            device::AcpiDeviceManager,
            table::{gtdt::GtdtManager, madt::MadtManager},
        },
        dtb::DtbManager,
        efi::{
            EFI_ACPI_2_0_TABLE_GUID, EFI_DTB_TABLE_GUID, EFI_PAGE_SIZE,
            memory_map::{EfiMemoryDescriptor, EfiMemoryType},
        },
    },
    file_manager::elf::{ELF_PROGRAM_HEADER_SEGMENT_LOAD, Elf64Header},
    initialization::{idle, init_task_ap, init_work_queue},
    manager_cluster::{CpuManagerCluster, get_cpu_manager_cluster, get_kernel_manager_cluster},
    memory_manager::{
        MemoryManager, alloc_pages, alloc_pages_with_physical_address,
        data_type::{Address, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress, VAddress},
        free_pages,
        memory_allocator::MemoryAllocator,
        physical_memory_manager::PhysicalMemoryManager,
        system_memory_manager::{SystemMemoryManager, get_physical_memory_manager},
        virtual_memory_manager::VirtualMemoryManager,
    },
    sync::spin_lock::Mutex,
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
    cpu_manager.cpu_id = cpu::mpidr_to_affinity(cpu::get_mpidr()) as usize;
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
    let mut entry_base_address = boot_information.memory_info.efi_memory_map_address;

    /* Free usable memory area */
    while entry_base_address
        < (boot_information.memory_info.efi_memory_map_address
            + boot_information.memory_info.efi_memory_map_size)
    {
        let entry = unsafe { &*(entry_base_address as *const EfiMemoryDescriptor) };
        entry_base_address += boot_information.memory_info.efi_descriptor_size;
        match entry.memory_type {
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

    let elf_header = unsafe { Elf64Header::from_ptr(&boot_information.elf_header_buffer) }.unwrap();
    for entry in elf_header.get_program_headers_iter(boot_information.elf_program_headers_address) {
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
    boot_information.efi_system_table.set_configuration_table(
        physical_address_to_direct_map(PAddress::new(
            boot_information.efi_system_table.get_configuration_table(),
        ))
        .to_usize(),
    );
    boot_information.font_address = boot_information.font_address.map(|a| {
        (
            (physical_address_to_direct_map(PAddress::new(a.0)).to_usize()),
            a.1,
        )
    });

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
    use core::mem::MaybeUninit;
    init_struct!(
        get_cpu_manager_cluster().interrupt_manager,
        InterruptManager::new()
    );
    let mut initialized = false;
    let mut distributor = MaybeUninit::uninit();
    let mut redistributor = MaybeUninit::uninit();

    get_cpu_manager_cluster().interrupt_manager.init();

    if acpi_available {
        /* Try to find with ACPI */
        let acpi_manager = get_kernel_manager_cluster().acpi_manager.lock().unwrap();
        if let Ok(mut gic_manager) = GicDistributor::new_with_acpi(&acpi_manager) {
            assert!(gic_manager.init(), "Failed to initialize GIC Distributor");
            redistributor.write(
                gic_manager
                    .new_redistributor_with_acpi(&acpi_manager)
                    .expect("Failed to initialize GIC Redistributor"),
            );
            distributor.write(gic_manager);
            initialized = true;
        }
    }

    if !initialized && dtb_available {
        /* Try to find with Devicetree */
        let dtb_manager = &get_kernel_manager_cluster().arch_depend_data.dtb_manager;
        if let Ok(mut gic_manager) = GicDistributor::new_with_dtb(dtb_manager) {
            assert!(gic_manager.init(), "Failed to initialize GIC Distributor");
            redistributor.write(
                gic_manager
                    .new_redistributor_with_dtb(dtb_manager)
                    .expect("Failed to initialize GIC Redistributor"),
            );
            distributor.write(gic_manager);
            initialized = true;
        }
    }

    assert!(initialized, "GIC is not available");

    init_struct!(
        get_cpu_manager_cluster()
            .arch_depend_data
            .gic_redistributor_manager,
        unsafe { redistributor.assume_init() }
    );
    init_struct!(
        get_kernel_manager_cluster().arch_depend_data.gic_manager,
        unsafe { distributor.assume_init() }
    );
    get_cpu_manager_cluster().interrupt_manager.init_ipi();
}

pub fn init_interrupt_ap(cpu_manager_cluster: &mut CpuManagerCluster) {
    let mut interrupt_manager = InterruptManager::new();
    let mut cpu_redistributor = None;
    interrupt_manager.init_ap();

    let acpi_manager = get_kernel_manager_cluster().acpi_manager.lock().unwrap();
    if acpi_manager.is_available() {
        cpu_redistributor = get_kernel_manager_cluster()
            .arch_depend_data
            .gic_manager
            .new_redistributor_with_acpi(&acpi_manager);
    }
    drop(acpi_manager);

    if cpu_redistributor.is_none() {
        /* Try to find with Devicetree */
        let dtb_manager = &get_kernel_manager_cluster().arch_depend_data.dtb_manager;
        cpu_redistributor = get_kernel_manager_cluster()
            .arch_depend_data
            .gic_manager
            .new_redistributor_with_dtb(dtb_manager);
    }

    init_struct!(
        cpu_manager_cluster
            .arch_depend_data
            .gic_redistributor_manager,
        cpu_redistributor.expect("GIC Redistributor is not available")
    );
    interrupt_manager.init_ipi();
    init_struct!(cpu_manager_cluster.interrupt_manager, interrupt_manager);
}

/// Init SerialPort
///
/// This function does not enable the interrupt.
pub fn init_serial_port(acpi_available: bool, dtb_available: bool) -> bool {
    if acpi_available
        && get_kernel_manager_cluster()
            .serial_port_manager
            .init_with_acpi()
    {
        true
    } else if dtb_available
        && get_kernel_manager_cluster()
            .serial_port_manager
            .init_with_dtb()
    {
        true
    } else {
        false
    }
}

/// Init AcpiManager without parsing AML
///
/// This function initializes ACPI Manager.
/// ACPI Manager will parse some tables and return.
/// If succeeded, this will move it into kernel_manager_cluster.
pub fn init_acpi_early_by_boot_information(boot_information: &BootInformation) -> bool {
    let mut acpi_manager = AcpiManager::new();
    let mut device_manager = AcpiDeviceManager::new();
    let set_manger = |a: AcpiManager, d: AcpiDeviceManager| {
        init_struct!(get_kernel_manager_cluster().acpi_manager, Mutex::new(a));
        init_struct!(get_kernel_manager_cluster().acpi_device_manager, d);
    };

    let mut rsdp_address: Option<usize> = None;
    for e in unsafe {
        boot_information
            .efi_system_table
            .get_configuration_table_slice()
    } {
        if e.vendor_guid == EFI_ACPI_2_0_TABLE_GUID {
            rsdp_address = Some(e.vendor_table);
            break;
        }
    }
    if rsdp_address.is_none() {
        set_manger(acpi_manager, device_manager);
        return false;
    }

    if !acpi_manager.init(rsdp_address.unwrap(), &mut device_manager) {
        pr_warn!("Failed to initialize ACPI.");
        set_manger(acpi_manager, device_manager);
        return false;
    }
    if let Some(e) = acpi_manager.create_acpi_event_manager() {
        init_struct!(get_kernel_manager_cluster().acpi_event_manager, e);
    } else {
        pr_err!("Failed to initialize ACPI Event Manager");
        set_manger(acpi_manager, device_manager);
        return false;
    }
    set_manger(acpi_manager, device_manager);
    true
}

/// Init Device Tree Blob Manager
pub fn init_dtb(boot_information: &BootInformation) -> bool {
    let mut dtb_manager = DtbManager::new();
    let mut dtb_address: Option<usize> = None;
    for e in unsafe {
        boot_information
            .efi_system_table
            .get_configuration_table_slice()
    } {
        if e.vendor_guid == EFI_DTB_TABLE_GUID {
            dtb_address = Some(e.vendor_table);
            break;
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

pub fn init_local_timer_and_system_counter(acpi_available: bool, dtb_available: bool) {
    init_struct!(
        get_cpu_manager_cluster().local_timer_manager,
        LocalTimerManager::new()
    );
    init_struct!(
        get_cpu_manager_cluster().arch_depend_data.generic_timer,
        GenericTimer::new()
    );
    init_struct!(
        get_kernel_manager_cluster().arch_depend_data.system_counter,
        SystemCounter::new()
    );

    let generic_timer = &mut get_cpu_manager_cluster().arch_depend_data.generic_timer;
    let system_counter = &mut get_kernel_manager_cluster().arch_depend_data.system_counter;
    let local_timer_manager = &mut get_cpu_manager_cluster().local_timer_manager;
    let mut initialized = false;
    if acpi_available
        && let Some(gtdt) = get_kernel_manager_cluster()
            .acpi_manager
            .lock()
            .unwrap()
            .get_table_manager()
            .get_table_manager::<GtdtManager>()
    {
        if let Some(cnt_base) = gtdt.get_cnt_control_base() {
            if let Err(e) = system_counter.init_cnt_ctl_base(PAddress::new(cnt_base)) {
                panic!("Failed to init System Counter: {:?}", e);
            }
        }
        let is_level_trigger;
        let interrupt_id;
        if cpu::get_current_el() == 2 {
            pr_info!("Using EL2 Physical Timer");
            is_level_trigger = (gtdt.get_el2_flags() & 1) == 0;
            interrupt_id = gtdt.get_el2_gsiv();
        } else {
            pr_info!("Using EL1 Timer");
            is_level_trigger = (gtdt.get_non_secure_el1_flags() & 1) == 0;
            interrupt_id = gtdt.get_non_secure_el1_gsiv();
        }

        generic_timer.init(true, is_level_trigger, interrupt_id, None);
        gtdt.delete_map();
        initialized = true;
    }

    if !initialized && dtb_available {
        let dtb_manager = &get_kernel_manager_cluster().arch_depend_data.dtb_manager;
        let mut previous_timer = None;
        while let Some(info) = dtb_manager.search_node(b"timer", previous_timer.as_ref()) {
            if dtb_manager.is_device_compatible(&info, b"arm,armv8-timer")
                && dtb_manager.is_node_operational(&info)
            {
                /* Found Usable timer */
                let clock_frequency = dtb_manager.get_property(&info, b"clock-frequency");
                let interrupt_index = if cpu::get_current_el() == 2 {
                    pr_info!("Using EL2 Physical Timer");
                    3
                } else {
                    pr_info!("Using EL1 Timer");
                    1
                };

                if let Some((interrupt_id, is_level_trigger)) =
                    read_interrupt_info_from_dtb(dtb_manager, &info, interrupt_index)
                {
                    generic_timer.init(
                        true,
                        is_level_trigger,
                        interrupt_id,
                        clock_frequency.and_then(|i| dtb_manager.read_property_as_u32(&i, 0)),
                    );
                    initialized = true;
                    break;
                } else {
                    pr_err!("Failed to get interrupt information");
                }
            }
            previous_timer = Some(info);
        }
    }

    assert!(initialized, "Failed to initialize Generic Timer");
    local_timer_manager.set_source_timer(generic_timer);
}

fn init_local_timer_ap() {
    init_struct!(
        get_cpu_manager_cluster().local_timer_manager,
        LocalTimerManager::new()
    );
    init_struct!(
        get_cpu_manager_cluster().arch_depend_data.generic_timer,
        GenericTimer::new()
    );
    get_cpu_manager_cluster()
        .arch_depend_data
        .generic_timer
        .init_ap(
            &get_kernel_manager_cluster()
                .boot_strap_cpu_manager
                .arch_depend_data
                .generic_timer,
        );
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
    let mut acpi_cpu_iter = None;
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
        acpi_cpu_iter =
            acpi_madt_manager.map(|m| m.get_generic_interrupt_controller_cpu_info_iter());
    } else if dtb_available {
        /* Do nothing */
    } else {
        pr_info!("Failed to get processors information");
        return;
    }

    let mut mpidr_iter = || {
        if acpi_available {
            acpi_cpu_iter.as_mut().unwrap().next()
        } else if dtb_available {
            let dtb_manager = &get_kernel_manager_cluster().arch_depend_data.dtb_manager;
            if let Some(n) = dtb_manager.search_node(b"cpu", dtb_cpu_node.as_ref()) {
                let mpidr = dtb_manager
                    .read_reg_property(&n, 0)
                    .map(|(mpidr, _)| mpidr as u64);
                dtb_cpu_node = Some(n);
                mpidr
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
        .expect("Failed to alloc stack for AP");
    let boot_data = [
        cpu::get_tcr(),
        cpu::get_ttbr1(),
        cpu::get_sctlr(),
        cpu::get_mair(),
        ap_temporary_interrupt_vector as *const fn() as usize as u64,
        (stack + stack_size).to_usize() as u64,
        ap_boot_main as *const fn() as usize as u64,
        0,
    ];

    unsafe {
        *((virtual_address.to_usize() + (ap_entry_end_address - ap_entry_address)) as *mut _) =
            boot_data
    };
    cpu::flush_data_cache_all();

    let bsp_mpidr = cpu::mpidr_to_affinity(cpu::get_mpidr());

    let mut num_of_cpu = 1usize;

    'ap_init_loop: while let Some(mpidr) = mpidr_iter() {
        if mpidr == bsp_mpidr {
            continue;
        }
        pr_info!("Boot the CPU (MPIDR: {mpidr:#X})");
        AP_BOOT_COMPLETE_FLAG.store(false, core::sync::atomic::Ordering::Relaxed);
        cpu::synchronize(VAddress::from(AP_BOOT_COMPLETE_FLAG.as_ptr()));
        let mut x0 = cpu::SMC_PSCI_CPU_ON;
        unsafe {
            cpu::smc_0(
                &mut x0,
                &mut mpidr.clone(),
                &mut (physical_address.to_usize() as u64).clone(),
                &mut 0,
                &mut 0,
                &mut 0,
                &mut 0,
                &mut 0,
                &mut 0,
                &mut 0,
                &mut 0,
                &mut 0,
                &mut 0,
                &mut 0,
                &mut 0,
                &mut 0,
                &mut 0,
                &mut 0,
            )
        }
        if x0 != 0 {
            pr_err!("Failed to startup the CPU: {x0:#X}");
            continue;
        }
        loop {
            cpu::synchronize(VAddress::from(AP_BOOT_COMPLETE_FLAG.as_ptr()));
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

pub extern "C" fn ap_boot_main() -> ! {
    /* Setup CPU Manager, it contains individual data of CPU */
    let cpu_manager = setup_cpu_manager_cluster(None);
    pr_info!(
        "Booted (CPU ID: {:#X}, CurrentEL: {})",
        cpu_manager.cpu_id,
        cpu::get_current_el()
    );

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
        .generic_timer
        .start_interrupt();
    idle()
}
