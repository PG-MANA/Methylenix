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
use crate::kernel::drivers::multiboot::MultiBootInformation;
use crate::kernel::drivers::{acpi::table::bgrt::BgrtManager, acpi::AcpiManager};
use crate::kernel::file_manager::elf::ELF_MACHINE_AMD64;
use crate::kernel::graphic_manager::GraphicManager;
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::data_type::{
    Address, MSize, MemoryOptionFlags, MemoryPermissionFlags, VAddress,
};
use crate::kernel::sync::spin_lock::Mutex;
use crate::kernel::tty::TtyManager;
use crate::{io_remap, mremap};

use core::mem;

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
    mem::forget(mem::replace(
        &mut get_kernel_manager_cluster().serial_port_manager,
        SerialPortManager::new(0x3F8 /* COM1 */),
    ));

    /* Load the multiboot information */
    let multiboot_information = MultiBootInformation::new(mbi_address, true);

    /* Setup BSP CPU Manager Cluster */
    mem::forget(mem::replace(
        &mut get_kernel_manager_cluster().cpu_list,
        PtrLinkedList::new(),
    ));
    setup_cpu_manager_cluster(Some(VAddress::new(
        &(get_kernel_manager_cluster().boot_strap_cpu_manager) as *const _ as usize,
    )));

    /* Init Graphic & TTY (for panic!) */
    mem::forget(mem::replace(
        &mut get_kernel_manager_cluster().graphic_manager,
        GraphicManager::new(),
    ));
    get_kernel_manager_cluster()
        .graphic_manager
        .init_by_multiboot_information(&multiboot_information.framebuffer_info);
    get_kernel_manager_cluster().graphic_manager.clear_screen();
    mem::forget(mem::replace(
        &mut get_kernel_manager_cluster().kernel_tty_manager,
        TtyManager::new(),
    ));
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

    /* Draw boot logo */
    draw_boot_logo();

    init_block_devices_and_file_system_early();
    init_ethernet_manager_early();

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

    crate::kernel::network_manager::dhcp::get_ipv4_address(0);
    idle()
}

fn idle() -> ! {
    loop {
        unsafe {
            cpu::idle();
        }
    }
}

fn draw_boot_logo() {
    let free_mapped_address = |address: usize| {
        if let Err(e) = get_kernel_manager_cluster()
            .kernel_memory_manager
            .free(VAddress::new(address))
        {
            pr_err!("Freeing the bitmap data of BGRT was failed: {:?}", e);
        }
    };
    let acpi_manager = get_kernel_manager_cluster().acpi_manager.lock().unwrap();

    let bgrt_manager = acpi_manager
        .get_table_manager()
        .get_table_manager::<BgrtManager>();
    drop(acpi_manager);
    if bgrt_manager.is_none() {
        pr_info!("ACPI does not have the BGRT information.");
        return;
    }
    let bgrt_manager = bgrt_manager.unwrap();
    let boot_logo_physical_address = bgrt_manager.get_bitmap_physical_address();
    let boot_logo_offset = bgrt_manager.get_image_offset();
    if boot_logo_physical_address.is_none() {
        pr_info!("Boot Logo is compressed.");
        return;
    }
    drop(bgrt_manager);

    let original_map_size = MSize::from(54);
    let result = io_remap!(
        boot_logo_physical_address.unwrap(),
        original_map_size,
        MemoryPermissionFlags::rodata(),
        MemoryOptionFlags::PRE_RESERVED
    );
    if result.is_err() {
        pr_err!(
            "Mapping the bitmap data of BGRT failed Err:{:?}",
            result.err()
        );
        return;
    }

    let boot_logo_address = result.unwrap().to_usize();
    pr_info!(
        "BGRT: {:#X} is mapped at {:#X}",
        boot_logo_physical_address.unwrap().to_usize(),
        boot_logo_address
    );
    drop(boot_logo_physical_address);

    if unsafe { *((boot_logo_address + 30) as *const u32) } != 0 {
        pr_info!("Boot logo is compressed");
        free_mapped_address(boot_logo_address);
        return;
    }
    let file_offset = unsafe { *((boot_logo_address + 10) as *const u32) };
    let bitmap_width = unsafe { *((boot_logo_address + 18) as *const u32) };
    let bitmap_height = unsafe { *((boot_logo_address + 22) as *const u32) };
    let bitmap_color_depth = unsafe { *((boot_logo_address + 28) as *const u16) };
    let aligned_bitmap_width =
        ((bitmap_width as usize * (bitmap_color_depth as usize / 8) - 1) & !3) + 4;

    let result = mremap!(
        boot_logo_address.into(),
        original_map_size,
        MSize::new(
            (aligned_bitmap_width * bitmap_height as usize * (bitmap_color_depth as usize >> 3))
                + file_offset as usize
        )
    );
    if result.is_err() {
        pr_err!(
            "Mapping the bitmap data of BGRT was failed:{:?}",
            result.err()
        );
        free_mapped_address(boot_logo_address);
        return;
    }

    if boot_logo_address != result.unwrap().to_usize() {
        pr_info!(
            "BGRT: {:#X} is remapped at {:#X}",
            boot_logo_address,
            result.unwrap().to_usize(),
        );
    }
    let boot_logo_address = result.unwrap();

    /* Adjust offset if it overflows from frame buffer. */
    let buffer_size = get_kernel_manager_cluster()
        .graphic_manager
        .get_frame_buffer_size();
    let boot_logo_offset = boot_logo_offset;
    let offset_x = if boot_logo_offset.0 + bitmap_width as usize > buffer_size.0 {
        (buffer_size.0 - bitmap_width as usize) / 2
    } else {
        boot_logo_offset.0
    };
    let offset_y = if boot_logo_offset.1 + bitmap_height as usize > buffer_size.1 {
        (buffer_size.1 - bitmap_height as usize) / 2
    } else {
        boot_logo_offset.1
    };

    get_kernel_manager_cluster().graphic_manager.write_bitmap(
        (boot_logo_address + MSize::new(file_offset as usize)).to_usize(),
        bitmap_color_depth as u8,
        bitmap_width as usize,
        bitmap_height as usize,
        offset_x,
        offset_y,
    );

    free_mapped_address(boot_logo_address.to_usize());
}

#[no_mangle]
pub extern "C" fn directboot_main(
    _info_address: usize,      /* DirectBoot Start Information */
    _kernel_code_segment: u16, /* Current segment is 8 */
    _user_code_segment: u16,
    _user_data_segment: u16,
) -> ! {
    mem::forget(mem::replace(
        &mut get_kernel_manager_cluster().serial_port_manager,
        SerialPortManager::new(0x3F8 /* COM1 */),
    ));
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
