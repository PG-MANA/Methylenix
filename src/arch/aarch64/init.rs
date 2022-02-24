//!
//! Initialization Functions
//!

use crate::arch::target_arch::boot_info::BootInformation;
use crate::arch::target_arch::context::{
    memory_layout::{
        get_direct_map_max_size, physical_address_to_direct_map, DIRECT_MAP_BASE_ADDRESS,
    },
    ContextManager,
};
use crate::arch::target_arch::device::{
    cpu,
    generic_timer::{GenericTimer, SystemCounter},
};
use crate::arch::target_arch::interrupt::{gic::GicManager, InterruptManager};
use crate::arch::target_arch::paging::{PAGE_MASK, PAGE_SIZE, PAGE_SIZE_USIZE};

use crate::kernel::block_device::BlockDeviceManager;
use crate::kernel::collections::ptr_linked_list::PtrLinkedListNode;
use crate::kernel::drivers::acpi::{
    device::AcpiDeviceManager,
    table::{gtdt::GtdtManager, mcfg::McfgManager},
    AcpiManager,
};
use crate::kernel::drivers::dtb::DtbManager;
use crate::kernel::drivers::efi::{
    memory_map::{EfiMemoryDescriptor, EfiMemoryType},
    EFI_ACPI_2_0_TABLE_GUID, EFI_DTB_TABLE_GUID, EFI_PAGE_SIZE,
};
use crate::kernel::drivers::pci::PciManager;
use crate::kernel::file_manager::{
    elf::{Elf64Header, ELF_PROGRAM_HEADER_SEGMENT_LOAD},
    FileManager,
};
use crate::kernel::manager_cluster::{
    get_cpu_manager_cluster, get_kernel_manager_cluster, CpuManagerCluster,
};
use crate::kernel::memory_manager::{
    data_type::{Address, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress, VAddress},
    memory_allocator::MemoryAllocator,
    physical_memory_manager::PhysicalMemoryManager,
    system_memory_manager::{get_physical_memory_manager, SystemMemoryManager},
    virtual_memory_manager::VirtualMemoryManager,
    MemoryManager,
};
use crate::kernel::sync::spin_lock::Mutex;
use crate::kernel::task_manager::{run_queue::RunQueue, TaskManager};
use crate::kernel::timer_manager::{GlobalTimerManager, LocalTimerManager};
use crate::{alloc_pages, alloc_pages_with_physical_address, free_pages};

use core::mem;
use core::sync::atomic::AtomicBool;

/// Memory Areas for PhysicalMemoryManager
static mut MEMORY_FOR_PHYSICAL_MEMORY_MANAGER: [u8; PAGE_SIZE_USIZE * 2] = [0; PAGE_SIZE_USIZE * 2];

pub static AP_BOOT_COMPLETE_FLAG: AtomicBool = AtomicBool::new(false);

/// Setup Per CPU struct
///
/// This function must be called on the cpu that is going to own returned manager.
pub fn setup_cpu_manager_cluster(
    cpu_manager_address: Option<VAddress>,
) -> &'static mut CpuManagerCluster {
    let cpu_manager_address = cpu_manager_address.unwrap_or_else(|| {
        /* ATTENTION: BSP must be sleeping. */
        get_kernel_manager_cluster()
            .boot_strap_cpu_manager /* Allocate from BSP Object Manager */
            .memory_allocator
            .kmalloc(MSize::new(mem::size_of::<CpuManagerCluster>()))
            .expect("Failed to alloc CpuManagerCluster")
    });
    let cpu_manager = unsafe { &mut *(cpu_manager_address.to_usize() as *mut CpuManagerCluster) };

    unsafe { cpu::set_cpu_base_address(cpu_manager as *const _ as u64) };
    mem::forget(mem::replace(
        &mut cpu_manager.list,
        PtrLinkedListNode::new(),
    ));
    get_kernel_manager_cluster()
        .cpu_list
        .insert_tail(&mut cpu_manager.list);
    cpu_manager.cpu_id = cpu::mpidr_to_affinity(unsafe { cpu::get_mpidr() }) as usize;
    cpu_manager
}

/// Init memory system based on boot information.
/// This function set up PhysicalMemoryManager which manages where is free
/// and VirtualMemoryManager which manages which process is using what area of virtual memory.
/// After that, this will set up MemoryManager.
/// If one of process is failed, this will panic.
/// This function returns new address of BootInformation.
pub fn init_memory_by_boot_information(boot_information: &BootInformation) -> BootInformation {
    let mut boot_information = boot_information.clone();
    /* Set up Physical Memory Manager */
    let mut physical_memory_manager = PhysicalMemoryManager::new();
    unsafe {
        physical_memory_manager.add_memory_entry_pool(
            &MEMORY_FOR_PHYSICAL_MEMORY_MANAGER as *const _ as usize,
            mem::size_of_val(&MEMORY_FOR_PHYSICAL_MEMORY_MANAGER),
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
                if let Err(e) = physical_memory_manager.free(start_address, size, true) {
                    pr_warn!("Failed to free memory: {:?}", e);
                }
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
                .to_end_address(PAddress::new(entry.physical_start as usize))
                .to_usize(),
            entry.memory_type
        );
    }

    /* Set up Virtual Memory Manager */
    let mut virtual_memory_manager = VirtualMemoryManager::new();
    virtual_memory_manager.init_system(
        DIRECT_MAP_BASE_ADDRESS + get_direct_map_max_size(),
        &mut physical_memory_manager,
    );
    mem::forget(mem::replace(
        &mut get_kernel_manager_cluster().system_memory_manager,
        SystemMemoryManager::new(physical_memory_manager),
    ));
    get_kernel_manager_cluster()
        .system_memory_manager
        .init_pools(&mut virtual_memory_manager);

    let elf_header = unsafe { Elf64Header::from_ptr(&boot_information.elf_header_buffer) }.unwrap();
    for entry in elf_header.get_program_header_iter(boot_information.elf_program_header_address) {
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
                if address == VAddress::new(virtual_address) {
                    continue;
                }
                pr_err!(
                    "Virtual Address is different from Physical Address: V:{:#X} P:{:#X}",
                    address.to_usize(),
                    virtual_address
                );
            }
            Err(e) => {
                pr_err!("Mapping ELF Section was failed: {:?}", e);
            }
        };
    }

    /* Set up Memory Manager */
    mem::forget(mem::replace(
        &mut get_kernel_manager_cluster().kernel_memory_manager,
        MemoryManager::new(virtual_memory_manager),
    ));

    /* Adjust Memory Pointer */
    boot_information.memory_info.efi_memory_map_address = physical_address_to_direct_map(
        PAddress::new(boot_information.memory_info.efi_memory_map_address),
    )
    .to_usize();
    boot_information.elf_program_header_address =
        physical_address_to_direct_map(PAddress::new(boot_information.elf_program_header_address))
            .to_usize();
    boot_information.efi_system_table.set_configuration_table(
        physical_address_to_direct_map(PAddress::new(
            boot_information.efi_system_table.get_configuration_table(),
        ))
        .to_usize(),
    );
    boot_information.font_address = boot_information.font_address.and_then(|a| {
        Some((
            (physical_address_to_direct_map(PAddress::new(a.0)).to_usize()),
            a.1,
        ))
    });

    /* Apply paging */
    get_kernel_manager_cluster()
        .kernel_memory_manager
        .set_paging_table();

    /* Set up Kernel Memory Alloc Manager */
    let mut memory_allocator = MemoryAllocator::new();
    memory_allocator
        .init()
        .expect("Failed to init MemoryAllocator");
    get_cpu_manager_cluster().memory_allocator = memory_allocator;

    boot_information
}

/// Init InterruptManager
pub fn init_interrupt(acpi_available: bool, dtb_available: bool) {
    mem::forget(mem::replace(
        &mut get_cpu_manager_cluster().interrupt_manager,
        InterruptManager::new(),
    ));
    get_cpu_manager_cluster().interrupt_manager.init();

    if acpi_available {
        if let Some(mut gic_manager) =
            GicManager::new_with_acpi(&get_kernel_manager_cluster().acpi_manager.lock().unwrap())
        {
            if !gic_manager.init_generic_interrupt_distributor() {
                panic!("Failed to init GIC");
            }
            let cpu_redistributor = gic_manager
                .init_redistributor()
                .expect("Failed to init GIC Redistributor");
            mem::forget(mem::replace(
                &mut get_cpu_manager_cluster()
                    .arch_depend_data
                    .gic_redistributor_manager,
                cpu_redistributor,
            ));
            mem::forget(mem::replace(
                &mut get_kernel_manager_cluster().arch_depend_data.gic_manager,
                gic_manager,
            ));
            get_cpu_manager_cluster().interrupt_manager.init_ipi();
            return;
        }
    }

    if dtb_available {
        if let Some(mut gic_manager) =
            GicManager::new_with_dtb(&get_kernel_manager_cluster().arch_depend_data.dtb_manager)
        {
            if !gic_manager.init_generic_interrupt_distributor() {
                panic!("Failed to init GIC");
            }
            let cpu_redistributor = gic_manager
                .init_redistributor()
                .expect("Failed to init GIC Redistributor");
            mem::forget(mem::replace(
                &mut get_cpu_manager_cluster()
                    .arch_depend_data
                    .gic_redistributor_manager,
                cpu_redistributor,
            ));
            mem::forget(mem::replace(
                &mut get_kernel_manager_cluster().arch_depend_data.gic_manager,
                gic_manager,
            ));
            get_cpu_manager_cluster().interrupt_manager.init_ipi();
            return;
        }
    }
    panic!("GIC is not available");
}

/// Init Work Queue
pub fn init_work_queue() {
    get_cpu_manager_cluster()
        .work_queue
        .init(&mut get_kernel_manager_cluster().task_manager);
}

/// Init SerialPort
///
///
pub fn init_serial_port(acpi_available: bool, dtb_available: bool) -> bool {
    if acpi_available {
        if get_kernel_manager_cluster()
            .serial_port_manager
            .init_with_acpi()
        {
            return true;
        }
    }
    if dtb_available {
        if get_kernel_manager_cluster()
            .serial_port_manager
            .init_with_dtb()
        {
            return true;
        }
    }
    return false;
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
        mem::forget(mem::replace(
            &mut get_kernel_manager_cluster().acpi_manager,
            Mutex::new(a),
        ));
        mem::forget(mem::replace(
            &mut get_kernel_manager_cluster().acpi_device_manager,
            d,
        ));
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
        mem::forget(mem::replace(
            &mut get_kernel_manager_cluster().acpi_event_manager,
            e,
        ));
    } else {
        pr_err!("Failed to initialize ACPI Event Manager");
        set_manger(acpi_manager, device_manager);
        return false;
    }
    set_manger(acpi_manager, device_manager);
    return true;
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
        mem::forget(mem::replace(
            &mut get_kernel_manager_cluster().arch_depend_data.dtb_manager,
            dtb_manager,
        ));
        return false;
    }

    if !dtb_manager.init(PAddress::new(dtb_address.unwrap())) {
        pr_warn!("Failed to initialize DTB.");
        mem::forget(mem::replace(
            &mut get_kernel_manager_cluster().arch_depend_data.dtb_manager,
            dtb_manager,
        ));
        return false;
    }
    mem::forget(mem::replace(
        &mut get_kernel_manager_cluster().arch_depend_data.dtb_manager,
        dtb_manager,
    ));
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
    /*if !super::device::acpi::setup_interrupt(&acpi_manager) {
        pr_err!("Cannot setup ACPI interrupt.");
        return false;
    }*/
    if !acpi_manager.setup_acpi_devices(&mut get_kernel_manager_cluster().acpi_device_manager) {
        pr_err!("Cannot setup ACPI devices.");
        return false;
    }
    if !acpi_manager.initialize_all_devices() {
        pr_err!("Cannot evaluate _STA/_INI methods.");
        return false;
    }
    /*get_kernel_manager_cluster()
    .acpi_event_manager
    .init_event_registers();*/
    if !acpi_manager.enable_acpi() {
        pr_err!("Cannot enable ACPI.");
        return false;
    }
    if !acpi_manager.enable_power_button(&mut get_kernel_manager_cluster().acpi_event_manager) {
        pr_err!("Cannot enable power button.");
        return false;
    }
    /*get_kernel_manager_cluster()
    .acpi_event_manager
    .enable_gpes();*/
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
            /* By SMC ? */
            return false;
        }
    } else {
        unimplemented!()
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

pub fn init_local_timer_and_system_counter(acpi_available: bool, dtb_available: bool) {
    mem::forget(mem::replace(
        &mut get_cpu_manager_cluster().local_timer_manager,
        LocalTimerManager::new(),
    ));
    mem::forget(mem::replace(
        &mut get_cpu_manager_cluster().arch_depend_data.generic_timer,
        GenericTimer::new(),
    ));
    mem::forget(mem::replace(
        &mut get_kernel_manager_cluster().arch_depend_data.system_counter,
        SystemCounter::new(),
    ));

    let generic_timer = &mut get_cpu_manager_cluster().arch_depend_data.generic_timer;
    let system_counter = &mut get_kernel_manager_cluster().arch_depend_data.system_counter;
    let local_timer_manager = &mut get_cpu_manager_cluster().local_timer_manager;
    let mut initialized = false;
    if acpi_available {
        if let Some(gtdt) = get_kernel_manager_cluster()
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
            generic_timer.init(
                true,
                (gtdt.get_non_secure_el1_flags() & 1) == 0,
                gtdt.get_non_secure_el1_gsiv(),
                None,
            );
            gtdt.delete_map();
            initialized = true;
        }
    }
    if !initialized && dtb_available {
        let dtb_manager = &get_kernel_manager_cluster().arch_depend_data.dtb_manager;
        let mut previous_timer = None;
        while let Some(info) = dtb_manager.search_node(b"timer", previous_timer.as_ref()) {
            if dtb_manager.is_device_compatible(&info, b"arm,armv8-timer")
                && dtb_manager.is_node_operational(&info)
            {
                /* Found Usable timer */
                if let Some(interrupts) =
                    dtb_manager.get_property(&info, &DtbManager::PROP_INTERRUPTS)
                {
                    let clock_frequency = dtb_manager.get_property(&info, b"clock-frequency");
                    let interrupts = dtb_manager.read_property_as_u32_array(&interrupts);
                    if interrupts.len() >= 3 * 2 {
                        pr_debug!(
                            "Generic Timer: {}",
                            if interrupts[3] == GicManager::DTB_GIC_PPI {
                                "PPI"
                            } else if interrupts[3] == GicManager::DTB_GIC_SPI {
                                "SPI"
                            } else {
                                "Unknown"
                            }
                        );
                        let interrupt_id = if interrupts[3] == GicManager::DTB_GIC_SPI {
                            interrupts[4] + GicManager::DTB_GIC_SPI_INTERRUPT_ID_OFFSET
                        } else {
                            interrupts[4]
                        };

                        generic_timer.init(
                            true,
                            (interrupts[5] & 0b1111) == 4,
                            interrupt_id,
                            clock_frequency.and_then(|i| dtb_manager.read_property_as_u32(&i)),
                        );
                        initialized = true;
                        break;
                    } else {
                        pr_err!(
                            "Interrupts cells are too small(Length: {:#X})",
                            interrupts.len()
                        );
                    }
                }
            }
            previous_timer = Some(info);
        }
    }

    if !initialized {
        panic!("Failed to initialize Generic Timer");
    }
    local_timer_manager.set_source_timer(generic_timer);
}

fn init_local_timer_ap() {
    mem::forget(mem::replace(
        &mut get_cpu_manager_cluster().local_timer_manager,
        LocalTimerManager::new(),
    ));
    mem::forget(mem::replace(
        &mut get_cpu_manager_cluster().arch_depend_data.generic_timer,
        GenericTimer::new(),
    ));
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

pub fn init_global_timer() {
    mem::forget(mem::replace(
        &mut get_kernel_manager_cluster().global_timer_manager,
        GlobalTimerManager::new(),
    ));
}

/// Init TaskManager
///
///
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
#[allow(dead_code)]
pub fn init_task_ap(idle_task: fn() -> !) {
    let mut run_queue = RunQueue::new();
    run_queue.init().expect("Failed to init RunQueue");

    get_kernel_manager_cluster()
        .task_manager
        .init_idle(idle_task, &mut run_queue);
    get_cpu_manager_cluster().run_queue = run_queue;
}

/// Initialize Block Device Manager and File System Manager
///
/// This function must be called before calling device scan functions.
pub fn init_block_devices_and_file_system_early() {
    mem::forget(mem::replace(
        &mut get_kernel_manager_cluster().block_device_manager,
        BlockDeviceManager::new(),
    ));
    mem::forget(mem::replace(
        &mut get_kernel_manager_cluster().file_manager,
        FileManager::new(),
    ));
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

/// Init APs
///
/// This function will setup multiple processors by using ACPI
/// This is in the development
pub fn init_multiple_processors_ap(acpi_available: bool, _dtb_available: bool) {
    if !acpi_available {
        unimplemented!()
    }
    /* Get available Local APIC IDs from ACPI */
    let madt_manager = get_kernel_manager_cluster()
        .arch_depend_data
        .gic_manager
        .get_madt_manager();
    if madt_manager.is_none() {
        pr_info!("ACPI does not have MADT.");
        return;
    }
    let mpidr_list_iter = madt_manager
        .unwrap()
        .get_generic_interrupt_controller_cpu_info_iter();

    /* Set BSP Local APIC ID into cpu_manager */

    /* Extern Assembly Symbols */
    extern "C" {
        /* device/cpu.rs */
        fn ap_entry();
        fn ap_entry_end();
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
    let stack = alloc_pages!(
        stack_size.to_order(None).to_page_order(),
        MemoryPermissionFlags::data()
    )
    .expect("Failed to alloc stack for AP");
    let boot_data = unsafe {
        [
            cpu::get_tcr(),
            cpu::get_ttbr1(),
            cpu::get_sctlr(),
            cpu::get_mair(),
            (stack + stack_size).to_usize() as u64,
            ap_boot_main as *const fn() as usize as u64,
        ]
    };

    unsafe {
        *((virtual_address.to_usize() + (ap_entry_end_address - ap_entry_address)) as *mut _) =
            boot_data
    };

    let bsp_mpidr = unsafe { cpu::mpidr_to_affinity(cpu::get_mpidr()) };

    let mut num_of_cpu = 1usize;

    'ap_init_loop: for mpidr in mpidr_list_iter {
        if mpidr == bsp_mpidr {
            continue;
        }
        pr_debug!("MPIDR: {:#X}", mpidr);
        AP_BOOT_COMPLETE_FLAG.store(false, core::sync::atomic::Ordering::Relaxed);
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
            pr_err!("Failed to startup CPU(Result of PSCI: {:#X})", x0);
            continue;
        }
        loop {
            if AP_BOOT_COMPLETE_FLAG.load(core::sync::atomic::Ordering::Relaxed) {
                num_of_cpu += 1;
                continue 'ap_init_loop;
            }
            core::hint::spin_loop();
        }
        //panic!("Fialed to setup CPU(MPIDR: {:#X})", mpidr);
    }

    let _ = free_pages!(virtual_address);
    let _ = free_pages!(stack);

    //madt_manager.release_memory_map();

    if num_of_cpu != 1 {
        pr_info!("Found {} CPUs", num_of_cpu);
    }
}

pub extern "C" fn ap_boot_main() -> ! {
    /* Setup CPU Manager, it contains individual data of CPU */
    let cpu_manager = setup_cpu_manager_cluster(None);

    /* Setup memory management system */
    let mut memory_allocator = MemoryAllocator::new();
    memory_allocator
        .init()
        .expect("Failed to init MemoryAllocator");
    mem::forget(mem::replace(
        &mut cpu_manager.memory_allocator,
        memory_allocator,
    ));

    /* Setup InterruptManager(including LocalApicManager) */
    let mut interrupt_manager = InterruptManager::new();
    interrupt_manager.init_ap();
    let cpu_redistributor = get_kernel_manager_cluster()
        .arch_depend_data
        .gic_manager
        .init_redistributor()
        .expect("Failed to init GIC Redistributor");
    mem::forget(mem::replace(
        &mut get_cpu_manager_cluster()
            .arch_depend_data
            .gic_redistributor_manager,
        cpu_redistributor,
    ));
    interrupt_manager.init_ipi();
    mem::forget(mem::replace(
        &mut cpu_manager.interrupt_manager,
        interrupt_manager,
    ));

    init_local_timer_ap();
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
    super::idle()
}
