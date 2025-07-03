//!
//! AArch64 Boot Entry
//!
//! Boot entry codes
//!

mod boot_info;
pub mod context;

pub mod device {
    pub mod acpi;
    pub mod cpu;
    pub mod generic_timer;
    pub mod pci;
    pub mod serial_port;
    pub mod text;
}

mod initialization;
pub mod interrupt;
pub mod paging;
pub mod system_call;

use self::boot_info::BootInformation;
use self::device::generic_timer::{GenericTimer, SystemCounter};
use self::device::serial_port::SerialPortManager;
use self::initialization::*;
use self::interrupt::gic::{GicDistributor, GicRedistributor};

use crate::kernel::collections::init_struct;
use crate::kernel::collections::ptr_linked_list::PtrLinkedList;
use crate::kernel::drivers::dtb::DtbManager;
pub use crate::kernel::file_manager::elf::ELF_MACHINE_AA64 as ELF_MACHINE_DEFAULT;
use crate::kernel::graphic_manager::{GraphicManager, font::FontType};
use crate::kernel::initialization::*;
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::data_type::VAddress;
use crate::kernel::tty::TtyManager;

pub struct ArchDependedKernelManagerCluster {
    dtb_manager: DtbManager,
    system_counter: SystemCounter,
    gic_manager: GicDistributor,
}

pub struct ArchDependedCpuManagerCluster {
    generic_timer: GenericTimer,
    gic_redistributor_manager: GicRedistributor,
    cpu_interface_number: u8,
}

pub const TARGET_ARCH_NAME: &str = "aarch64";

#[unsafe(no_mangle)]
extern "C" fn boot_main(boot_information: *const BootInformation) -> ! {
    let boot_information = unsafe { &*boot_information };

    /* Initialize Kernel TTY (Early) */
    init_struct!(
        get_kernel_manager_cluster().kernel_tty_manager[0],
        TtyManager::new()
    );
    init_struct!(
        get_kernel_manager_cluster().kernel_tty_manager[1],
        TtyManager::new()
    );

    /* Init Early Serial Port */
    init_struct!(
        get_kernel_manager_cluster().serial_port_manager,
        SerialPortManager::new()
    );
    get_kernel_manager_cluster().kernel_tty_manager[0]
        .open(&get_kernel_manager_cluster().serial_port_manager);

    /* Setup BSP cpu manager */
    init_struct!(get_kernel_manager_cluster().cpu_list, PtrLinkedList::new());
    setup_cpu_manager_cluster(Some(VAddress::from(
        &get_kernel_manager_cluster().boot_strap_cpu_manager as *const _,
    )));

    /* Initialize Memory System */
    let boot_information = init_memory_by_boot_information(boot_information);

    /* Initialize ACPI and DTB */
    let acpi_available = init_acpi_early_by_boot_information(&boot_information);
    let dtb_available = init_dtb(&boot_information);
    if !acpi_available && !dtb_available {
        panic!("Neither ACPI nor DTB is available");
    }

    /* Detect serial port*/
    init_serial_port(acpi_available, dtb_available);

    /* Initialize Graphic */
    init_struct!(
        get_kernel_manager_cluster().graphic_manager,
        GraphicManager::new()
    );
    if let Some(graphic_info) = &boot_information.graphic_info {
        get_kernel_manager_cluster()
            .graphic_manager
            .init_by_efi_information(
                graphic_info.frame_buffer_base,
                graphic_info.frame_buffer_size,
                &graphic_info.info,
            );
        get_kernel_manager_cluster()
            .graphic_manager
            .set_frame_buffer_memory_permission();
        if let Some((address, size)) = boot_information.font_address {
            get_kernel_manager_cluster().graphic_manager.load_font(
                VAddress::new(address),
                size,
                FontType::Pff2,
            );
        }
    }
    get_kernel_manager_cluster().kernel_tty_manager[1]
        .open(&get_kernel_manager_cluster().graphic_manager);

    kprintln!("{} Version {}", crate::OS_NAME, crate::OS_VERSION);
    pr_info!(
        "Booted from AArch64 BootLoader: ACPI: {} DTB: {}",
        acpi_available,
        dtb_available
    );

    /* Init interrupt */
    init_interrupt(acpi_available, dtb_available);

    /* Init Timers */
    init_local_timer_and_system_counter(acpi_available, dtb_available);
    init_global_timer();

    /* Init the task management system */
    init_task(main_arch_depend_initialization_process, idle);

    /* Setup work queue system */
    init_work_queue();

    /* Setup APs if the processor is multicore-processor */
    init_multiple_processors_ap(acpi_available, dtb_available);

    /* Switch to main process */
    get_cpu_manager_cluster().run_queue.start()
    /* Never return to here */
}

fn main_arch_depend_initialization_process() -> ! {
    /* Interrupt is enabled */

    /* Start Timer*/
    get_cpu_manager_cluster()
        .arch_depend_data
        .generic_timer
        .start_interrupt();

    if !get_kernel_manager_cluster()
        .serial_port_manager
        .setup_interrupt()
    {
        pr_err!("Failed to setup interrupt of SerialPort");
    }

    pr_info!("All arch-depend initializations are done!");
    main_initialization_process()
}
