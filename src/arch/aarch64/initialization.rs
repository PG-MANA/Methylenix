//!
//! The arch-depended functions for initialization
//!
//! This module includes init codes for devices, memory, and task system.
//! This module is called by boot function.
//!

use crate::arch::target_arch::{
    context::ContextManager,
    device::{
        cpu,
        generic_timer::{GenericTimer, SystemCounter},
    },
    interrupt::{InterruptManager, gic::GicDistributor, gic::read_interrupt_info_from_dtb},
    paging::{PAGE_SIZE, PAGE_SIZE_USIZE},
};

use crate::kernel::{
    collections::{init_struct, ptr_linked_list::PtrLinkedListNode},
    drivers::{
        acpi::table::{gtdt::GtdtManager, madt::MadtManager},
        boot_information::BootInformation,
        dtb::DtbManager,
        efi::EFI_DTB_TABLE_GUID,
    },
    initialization::{idle, init_task_ap, init_work_queue},
    manager_cluster::{CpuManagerCluster, get_cpu_manager_cluster, get_kernel_manager_cluster},
    memory_manager::{
        alloc_pages, alloc_pages_with_physical_address,
        data_type::{Address, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress, VAddress},
        free_pages,
        memory_allocator::MemoryAllocator,
        physical_memory_manager::PhysicalMemoryManager,
    },
    task_manager::{TaskManager, run_queue::RunQueue},
    timer_manager::{IntervalTimer, LocalTimerManager},
};

use core::arch::global_asm;
use core::sync::atomic::AtomicBool;

pub static AP_BOOT_COMPLETE_FLAG: AtomicBool = AtomicBool::new(false);

/// Called from [`crate::kernel::initialization::init_memory_by_boot_information`]
pub fn reserve_arch_depended_memory(_pm_manager: &mut PhysicalMemoryManager) {}

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
    cpu_manager.cpu_id = cpu::mpidr_to_affinity(cpu::get_mpidr()) as usize;
    cpu_manager
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
        if let Some(cnt_base) = gtdt.get_cnt_control_base()
            && let Err(e) = system_counter.init_cnt_ctl_base(PAddress::new(cnt_base))
        {
            panic!("Failed to init System Counter: {:?}", e);
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
    get_cpu_manager_cluster()
        .local_timer_manager
        .set_source_timer(&get_cpu_manager_cluster().arch_depend_data.generic_timer);
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
        ap_entry_end_address - ap_entry_address
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
    assert!(
        MSize::new((ap_entry_end_address - ap_entry_address) + size_of_val(&boot_data)) < PAGE_SIZE
    );

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
        cpu::synchronize(AP_BOOT_COMPLETE_FLAG.as_ptr());
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

global_asm!(
    "
.global     ap_entry, ap_entry_end
.section    .text
.type       ap_entry, %function
.align      2
ap_entry:
    mrs x2, currentel
    lsr x2, x2, 2
    cmp x2, 2
    b.ne 3f
    /* EL2 */
    mov x3, (0b11 << 24) /* SMEN */ | (0b11 << 20) /* FPEN */ | (0b11 << 16) /* ZEN */
    msr cptr_el2, x3
    /* is FEAT_VHE supported? */
    mrs x2, id_aa64mmfr1_el1
    tst x2, (0b1111 << 8)
    b.ne 2f
    /*  FEAT_VHE is not supported */
    /* Jump to EL1 */
    mov x3, (1 << 11) | (1 << 10) | (1 << 9) | (1 << 8) | (1 << 1) | (1 << 0)
    msr cnthctl_el2, x3
    adr x2, 3f
    msr elr_el2, x2
    mov x3, 0xC5
    msr spsr_el2, x3
    mov x2, (1 << 47) | (1 << 41) | (1 << 40)
    orr x2, x2, (1 << 31)
    orr x2, x2, (1 << 19)
    msr hcr_el2, x2
    eret
2:
    /* FEAT_VHE is supported */
    mov x2, (1 << 31) /* RW */ | (1 << 27) /* TGE */
    orr x2, x2, (1 << 34) /* E2H */
    msr hcr_el2, x2
    isb
3:
    /* EL1 or EL2(E2H) */
    mrs x6, DAIF
    orr x6, x6, (1 << 6) | (1 << 7)
    msr DAIF, x6
    isb
    adr x30, ap_entry_end
    ldp  x1,  x2, [x30, #(16 * 0)] /* x1: TCR_EL1,   x2: TTBR1_EL1 */
    ldp  x3,  x4, [x30, #(16 * 1)] /* x3: SCTLR_EL1, x4: MAIR_EL1 */
    ldp  x5,  x6, [x30, #(16 * 2)] /* x5: VBAR_EL1,  x6: Stack Pointer */
    ldp  x7, xzr, [x30, #(16 * 3)] /* x7: Entry Point */
    msr tcr_el1,    x1
    msr ttbr1_el1,  x2
    msr mair_el1,   x4
    msr vbar_el1,   x5
    mov sp,         x6
    isb
    msr sctlr_el1,  x3
    br  x7
.align  4
ap_entry_end:
.size   ap_entry, ap_entry_end - ap_entry

.global     ap_temporary_interrupt_vector
.balign     0x800
ap_temporary_interrupt_vector:
/* synchronous_current_el_stack_pointer_0 */
    msr elr_el1, x7
    eret

.balign 0x080
/* irq_current_el_stack_pointer_0 */
    b   ap_temporary_interrupt_vector

.balign 0x080
/* fiq_current_el_stack_pointer_0 */
    b   ap_temporary_interrupt_vector

.balign 0x080
/* s_error_current_el_stack_pointer_0 */
    b   ap_temporary_interrupt_vector

.balign 0x080
/* synchronous_current_el_stack_pointer_x */
    msr elr_el1, x7
    eret

.balign     0x800
.size   ap_temporary_interrupt_vector, . - ap_temporary_interrupt_vector
"
);

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
