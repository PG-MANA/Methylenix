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

use self::device::cpu;
use self::device::io_apic::IoApicManager;
use self::device::local_apic_timer::LocalApicTimer;
use self::device::serial_port::SerialPortManager;
use self::initialization::multiboot::{init_graphic, init_memory_by_multiboot_information};
use self::initialization::*;

use crate::kernel::collections::init_struct;
use crate::kernel::collections::ptr_linked_list::PtrLinkedList;
use crate::kernel::drivers::acpi::AcpiManager;
use crate::kernel::drivers::multiboot::MultiBootInformation;
pub use crate::kernel::file_manager::elf::ELF_MACHINE_AMD64 as ELF_MACHINE_DEFAULT;
use crate::kernel::graphic_manager::GraphicManager;
use crate::kernel::initialization::*;
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::data_type::VAddress;
use crate::kernel::sync::spin_lock::Mutex;
use crate::kernel::tty::TtyManager;

pub struct ArchDependedCpuManagerCluster {
    pub local_apic_timer: LocalApicTimer,
    pub self_pointer: usize,
}

pub struct ArchDependedKernelManagerCluster {
    pub io_apic_manager: Mutex<IoApicManager>,
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

    /* Load the multiboot information */
    let multiboot_information = MultiBootInformation::new(mbi_address, true);

    /* Setup BSP CPU Manager Cluster */
    init_struct!(get_kernel_manager_cluster().cpu_list, PtrLinkedList::new());
    setup_cpu_manager_cluster(Some(VAddress::from(
        &(get_kernel_manager_cluster().boot_strap_cpu_manager) as *const _,
    )));

    /* Init Graphic */
    init_struct!(
        get_kernel_manager_cluster().graphic_manager,
        GraphicManager::new()
    );
    get_kernel_manager_cluster()
        .graphic_manager
        .init_by_multiboot_information(&multiboot_information.framebuffer_info);
    get_kernel_manager_cluster().graphic_manager.clear_screen();
    get_kernel_manager_cluster().kernel_tty_manager[1]
        .open(&get_kernel_manager_cluster().graphic_manager);

    kprintln!("{} Version {}", crate::OS_NAME, crate::OS_VERSION);
    pr_info!(
        "Booted from {}, cmd line: {}",
        multiboot_information.boot_loader_name,
        multiboot_information.boot_cmd_line
    );

    /* Init the memory management system */
    let multiboot_information = init_memory_by_multiboot_information(multiboot_information);
    if !get_kernel_manager_cluster()
        .graphic_manager
        .set_frame_buffer_memory_permission()
    {
        panic!("Cannot map memory for frame buffer");
    }

    /* Set up graphic */
    init_graphic(&multiboot_information);

    /* Init interrupt */
    init_interrupt(kernel_cs, user_cs);

    /* Setup Serial Port */
    get_kernel_manager_cluster().serial_port_manager.init();

    /* Setup ACPI */
    if let Some(rsdp_address) = multiboot_information.new_acpi_rsdp_ptr {
        if !init_acpi_early(rsdp_address) {
            pr_err!("Failed Init ACPI.");
        }
    } else if multiboot_information.old_acpi_rsdp_ptr.is_some() {
        pr_warn!("ACPI 1.0 is not supported.");
        get_kernel_manager_cluster().acpi_manager = Mutex::new(AcpiManager::new());
    } else {
        pr_warn!("ACPI is not available.");
        get_kernel_manager_cluster().acpi_manager = Mutex::new(AcpiManager::new());
    }

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
        .start_interrupt(
            get_cpu_manager_cluster()
                .interrupt_manager
                .get_local_apic_manager(),
        );

    main_initialization_process()
}
