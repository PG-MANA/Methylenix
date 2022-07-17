//!
//! x86_64 Boot Entry
//!
//! Boot entry code from assembly.

#[macro_use]
pub mod interrupt;
pub mod boot;
pub mod context;
pub mod device;
mod init;
pub mod paging;
pub mod system_call;

use self::device::cpu;
use self::device::io_apic::IoApicManager;
use self::device::local_apic_timer::LocalApicTimer;
use self::device::serial_port::SerialPortManager;
use self::init::multiboot::{init_graphic, init_memory_by_multiboot_information};
use self::init::*;

use crate::kernel::application_loader;
use crate::kernel::collections::ptr_linked_list::PtrLinkedList;
use crate::kernel::drivers::acpi::AcpiManager;
use crate::kernel::drivers::multiboot::MultiBootInformation;
use crate::kernel::file_manager::elf::ELF_MACHINE_AMD64;
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

#[no_mangle]
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

    /* Set SerialPortManager(send only) for early debug */
    init_struct!(
        get_kernel_manager_cluster().serial_port_manager,
        SerialPortManager::new(0x3F8 /* COM1 */)
    );

    /* Load the multiboot information */
    let multiboot_information = MultiBootInformation::new(mbi_address, true);

    /* Setup BSP CPU Manager Cluster */
    init_struct!(get_kernel_manager_cluster().cpu_list, PtrLinkedList::new());
    setup_cpu_manager_cluster(Some(VAddress::new(
        &(get_kernel_manager_cluster().boot_strap_cpu_manager) as *const _ as usize,
    )));

    /* Init Graphic & TTY (for panic!) */
    init_struct!(
        get_kernel_manager_cluster().graphic_manager,
        GraphicManager::new()
    );
    get_kernel_manager_cluster()
        .graphic_manager
        .init_by_multiboot_information(&multiboot_information.framebuffer_info);
    get_kernel_manager_cluster().graphic_manager.clear_screen();
    init_struct!(
        get_kernel_manager_cluster().kernel_tty_manager,
        TtyManager::new()
    );
    get_kernel_manager_cluster()
        .kernel_tty_manager
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
    init_task(kernel_cs, user_cs, user_ss, main_process, idle);

    /* Setup work queue system */
    init_work_queue();

    /* Setup APs if the processor is multicore-processor */
    init_multiple_processors_ap();

    /* Switch to main process */
    get_cpu_manager_cluster().run_queue.start()
    /* Never return to here */
}

pub fn general_protection_exception_handler(e_code: usize) -> ! {
    panic!("General Protection Exception \nError Code:0x{:X}", e_code);
}

fn main_process() -> ! {
    /* Interrupt is enabled */

    get_cpu_manager_cluster()
        .arch_depend_data
        .local_apic_timer
        .start_interrupt(
            get_cpu_manager_cluster()
                .interrupt_manager
                .get_local_apic_manager(),
        );
    pr_info!("All initializations are done!");

    draw_boot_logo();

    init_block_devices_and_file_system_early();
    init_network_manager_early();

    if init_pci_early() {
        if !init_acpi_later() {
            pr_err!("Cannot init ACPI devices.");
        }
    } else {
        pr_err!("Cannot init PCI Manager.");
    }

    if !init_pci_later() {
        pr_err!("Cannot init PCI devices.");
    }

    init_block_devices_and_file_system_later();

    let _ = crate::kernel::network_manager::dhcp::get_ipv4_address_sync(0);

    /* Test */
    const ENVIRONMENT_VARIABLES: [(&str, &str); 3] = [
        ("OSTYPE", crate::OS_NAME),
        ("OSVERSION", crate::OS_VERSION),
        ("TARGET", "x86_64"),
    ];
    let _ = application_loader::load_and_execute(
        "/OS/FILES/APP",
        &["Arg1", "Arg2", "Arg3"],
        &ENVIRONMENT_VARIABLES,
        ELF_MACHINE_AMD64,
    );

    idle()
}

#[no_mangle]
pub extern "C" fn directboot_main(
    _info_address: usize,      /* DirectBoot Start Information */
    _kernel_code_segment: u16, /* Current segment is 8 */
    _user_code_segment: u16,
    _user_data_segment: u16,
) -> ! {
    init_struct!(
        get_kernel_manager_cluster().serial_port_manager,
        SerialPortManager::new(0x3F8 /* COM1 */)
    );
    get_kernel_manager_cluster()
        .serial_port_manager
        .send_str("Booted from DirectBoot\n");
    loop {
        unsafe {
            cpu::halt();
        }
    }
}

#[no_mangle]
pub extern "C" fn unknown_boot_main() -> ! {
    SerialPortManager::new(0x3F8).send_str("Unknown Boot System!");
    loop {
        unsafe { cpu::halt() };
    }
}
