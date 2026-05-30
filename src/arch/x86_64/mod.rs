//!
//! x86_64 Boot Routines
//!

pub mod boot;
pub mod context;
pub mod device;
mod initialization;
pub mod interrupt;
pub mod paging;
pub mod system_call;

use self::device::{
    cpu, io_apic::IoApicManager, local_apic_timer::LocalApicTimer, serial_port::SerialPortManager,
};
use self::initialization::{multiboot::*, *};

use crate::kernel::collections::{init_struct, ptr_linked_list::PtrLinkedList};
use crate::kernel::drivers::acpi::{AcpiManager, RSDP};
use crate::kernel::drivers::boot_information::BootInformation;
use crate::kernel::drivers::device::vga_text::VgaTextDriver;
use crate::kernel::drivers::multiboot::MultiBootInformation;
use crate::kernel::initialization::*;
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::data_type::VAddress;
use crate::kernel::sync::spin_lock::Mutex;
use crate::kernel::timer_manager::IntervalTimer;
use crate::kernel::tty::TtyManager;

pub use self::initialization::reserve_arch_depended_memory;
pub use crate::kernel::file_manager::elf::ELF_MACHINE_AMD64 as ELF_MACHINE_DEFAULT;

pub struct ArchDependedCpuManagerCluster {
    pub local_apic_timer: LocalApicTimer,
    pub self_pointer: usize,
}

pub struct ArchDependedKernelManagerCluster {
    pub io_apic_manager: Mutex<IoApicManager>,
    vga_text_driver: VgaTextDriver,
}

pub const TARGET_ARCH_NAME: &str = "x86_64";

#[unsafe(no_mangle)]
pub extern "C" fn multiboot_main(
    mbi_address: usize, /* MultiBoot Information */
    kernel_cs: u16,
    user_cs: u16,
    user_ss: u16,
) -> ! {
    /* Enable fxsave and fxrstor and fs/gs_base */
    unsafe {
        cpu::enable_sse();
        cpu::enable_fs_gs_base();
    }

    /* Initialize Kernel TTY (Early) */
    init_struct!(
        get_kernel_manager_cluster().kernel_tty_manager[0],
        TtyManager::new()
    );
    init_struct!(
        get_kernel_manager_cluster().kernel_tty_manager[1],
        TtyManager::new()
    );
    /* Initialize Serial Port */
    init_struct!(
        get_kernel_manager_cluster().serial_port_manager,
        SerialPortManager::new(0x3F8 /* COM1 */)
    );
    get_kernel_manager_cluster().kernel_tty_manager[0]
        .open(&get_kernel_manager_cluster().serial_port_manager);

    /* Setup BSP CPU Manager Cluster */
    init_struct!(get_kernel_manager_cluster().cpu_list, PtrLinkedList::new());
    setup_cpu_manager_cluster(Some(VAddress::from(
        &(get_kernel_manager_cluster().boot_strap_cpu_manager) as *const _,
    )));

    /* Load the multiboot information */
    let multiboot_information = MultiBootInformation::new(mbi_address, true);

    /* Init Graphic */
    init_graphic_early(&multiboot_information);

    kprintln!("{} Version {}", crate::OS_NAME, crate::OS_VERSION);
    pr_info!(
        "Booted from {}, cmd line: {}",
        multiboot_information.boot_loader_name,
        multiboot_information.boot_cmd_line
    );

    /* Init the memory management system */
    let multiboot_information = init_memory_by_multiboot_information(multiboot_information);

    /* Init interrupt */
    init_interrupt(kernel_cs, user_cs);

    /* Set up graphic */
    init_graphic(&multiboot_information);

    /* Setup ACPI */
    if let Some(rsdp_address) = multiboot_information.new_acpi_rsdp_ptr {
        /* `rsdp_address` points a copy of RSDP in the multiboot_information */
        let rsdp = unsafe { &*(rsdp_address as *const RSDP) };
        if !init_acpi_early(rsdp) {
            pr_err!("Failed Init ACPI.");
        }
    } else if multiboot_information.old_acpi_rsdp_ptr.is_some() {
        pr_warn!("ACPI 1.0 is not supported.");
        get_kernel_manager_cluster().acpi_manager = Mutex::new(AcpiManager::new());
    } else {
        pr_warn!("ACPI is not available.");
        get_kernel_manager_cluster().acpi_manager = Mutex::new(AcpiManager::new());
    }

    boot_latter_half(kernel_cs, user_cs, user_ss)
}

#[unsafe(no_mangle)]
pub extern "C" fn boot_main(
    boot_information: *const BootInformation,
    kernel_cs: u16,
    user_cs: u16,
    user_ss: u16,
) -> ! {
    let mut boot_information = unsafe { &*boot_information }.clone();
    /* Enable fxsave and fxrstor and fs/gs_base */
    unsafe {
        cpu::enable_sse();
        cpu::enable_fs_gs_base();
    }

    /* Initialize Kernel TTY (Early) */
    init_struct!(
        get_kernel_manager_cluster().kernel_tty_manager[0],
        TtyManager::new()
    );
    init_struct!(
        get_kernel_manager_cluster().kernel_tty_manager[1],
        TtyManager::new()
    );
    /* Initialize Serial Port */
    init_struct!(
        get_kernel_manager_cluster().serial_port_manager,
        SerialPortManager::new(0x3F8 /* COM1 */)
    );
    get_kernel_manager_cluster().kernel_tty_manager[0]
        .open(&get_kernel_manager_cluster().serial_port_manager);

    /* Setup BSP CPU Manager Cluster */
    init_struct!(get_kernel_manager_cluster().cpu_list, PtrLinkedList::new());
    setup_cpu_manager_cluster(Some(VAddress::from(
        &(get_kernel_manager_cluster().boot_strap_cpu_manager) as *const _,
    )));

    /* Initialize Memory System */
    init_memory_by_boot_information(&mut boot_information);

    /* Init interrupt */
    init_interrupt(kernel_cs, user_cs);

    /* Initialize Graphic */
    if init_graphic_by_boot_information(&boot_information)
        && init_graphic_font_by_boot_information(&boot_information)
    {
        get_kernel_manager_cluster().kernel_tty_manager[1]
            .open(&get_kernel_manager_cluster().graphic_manager);
    }

    kprintln!("{} Version {}", crate::OS_NAME, crate::OS_VERSION);

    /* Initialize ACPI */
    assert!(
        init_acpi_early_by_boot_information(&boot_information),
        "ACPI is not available."
    );

    boot_latter_half(kernel_cs, user_cs, user_ss)
}

fn boot_latter_half(kernel_cs: u16, user_cs: u16, user_ss: u16) -> ! {
    /* Setup Serial Port */
    get_kernel_manager_cluster().serial_port_manager.init();

    /* Init Timers */
    init_local_timer();
    init_global_timer();

    /* Init the task management system */
    init_task(
        kernel_cs,
        user_cs,
        user_ss,
        main_arch_depend_initialization_process,
        idle,
    );

    init_work_queue();

    wake_up_application_processors();

    /* Switch to the main process */
    get_cpu_manager_cluster().run_queue.start()
    /* Never return to here */
}

pub fn general_protection_exception_handler(e_code: usize) -> ! {
    panic!("General Protection Exception \nError Code:0x{:X}", e_code);
}

fn main_arch_depend_initialization_process() -> ! {
    get_cpu_manager_cluster()
        .arch_depend_data
        .local_apic_timer
        .start_interrupt();

    if !init_pci_early(
        Some(&get_kernel_manager_cluster().acpi_manager.lock().unwrap()),
        None,
    ) {
        pr_err!("Cannot init PCI Manager.");
    }

    main_initialization_process()
}
