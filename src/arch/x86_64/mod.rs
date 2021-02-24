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

use self::device::cpu;
use self::device::io_apic::IoApicManager;
use self::device::local_apic_timer::LocalApicTimer;
use self::device::serial_port::SerialPortManager;
use self::init::multiboot::{init_graphic, init_memory_by_multiboot_information};
use self::init::*;
use self::interrupt::{idt::GateDescriptor, InterruptManager};

use crate::kernel::drivers::acpi::AcpiManager;
use crate::kernel::drivers::multiboot::MultiBootInformation;
use crate::kernel::graphic_manager::GraphicManager;
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::data_type::{Address, MSize, MemoryPermissionFlags, VAddress};
use crate::kernel::memory_manager::object_allocator::ObjectAllocator;
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
    mbi_address: usize,       /* MultiBoot Information */
    kernel_code_segment: u16, /* Current segment is 8 */
    user_code_segment: u16,
    user_data_segment: u16,
) -> ! {
    /* Enable fxsave and fxrstor and fs/gs_base */
    unsafe {
        cpu::enable_sse();
        cpu::enable_fs_gs_base();
    }

    /* Set SerialPortManager(send only) for early debug */
    get_kernel_manager_cluster().serial_port_manager =
        SerialPortManager::new(0x3F8 /* COM1 */);

    /* Load the multiboot information */
    let multiboot_information = MultiBootInformation::new(mbi_address, true);

    /* Setup BSP CPU Manager Cluster */
    setup_cpu_manager_cluster(Some(VAddress::new(
        &(get_kernel_manager_cluster().boot_strap_cpu_manager) as *const _ as usize,
    )));

    /* Init Graphic & TTY (for panic!) */
    get_kernel_manager_cluster().graphic_manager = GraphicManager::new();
    get_kernel_manager_cluster()
        .graphic_manager
        .init(&multiboot_information.framebuffer_info);
    get_kernel_manager_cluster().graphic_manager.clear_screen();
    get_kernel_manager_cluster().kernel_tty_manager = TtyManager::new();
    get_kernel_manager_cluster()
        .kernel_tty_manager
        .open(&get_kernel_manager_cluster().graphic_manager);

    kprintln!("{}", crate::OS_NAME);
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
    init_interrupt(kernel_code_segment);

    /* Setup Serial Port */
    get_kernel_manager_cluster().serial_port_manager.init();

    /* Setup ACPI */
    if let Some(rsdp_address) = multiboot_information.new_acpi_rsdp_ptr {
        if !init_acpi_early(rsdp_address) {
            pr_err!("Failed Init ACPI.");
            get_kernel_manager_cluster().acpi_manager = Mutex::new(AcpiManager::new());
        }
    } else if multiboot_information.old_acpi_rsdp_ptr.is_some() {
        pr_warn!("ACPI 1.0 is not supported.");
        get_kernel_manager_cluster().acpi_manager = Mutex::new(AcpiManager::new());
    } else {
        pr_warn!("ACPI is not available.");
        get_kernel_manager_cluster().acpi_manager = Mutex::new(AcpiManager::new());
    }

    /* Init Local APIC Timer */
    get_cpu_manager_cluster().arch_depend_data.local_apic_timer = init_timer();

    /* Init the task management system */
    init_task(
        kernel_code_segment,
        user_code_segment,
        user_data_segment,
        main_process,
        idle,
    );

    /* Setup the interrupt work queue system */
    init_interrupt_work_queue_manager();

    /* Setup APs if the processor is multicore-processor */
    init_multiple_processors_ap();

    /* Switch to main process */
    get_cpu_manager_cluster().run_queue_manager.start()
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
            get_kernel_manager_cluster()
                .boot_strap_cpu_manager
                .interrupt_manager
                .lock()
                .unwrap()
                .get_local_apic_manager(),
        );
    pr_info!("All init are done!");

    /* Draw boot logo */
    draw_boot_logo();

    kprintln!("{} Version {}", crate::OS_NAME, crate::OS_VERSION);

    if !init_acpi_later() {
        pr_err!("Cannot init ACPI devices.");
    }

    loop {
        get_cpu_manager_cluster().run_queue_manager.sleep();
        while let Some(c) = get_kernel_manager_cluster()
            .serial_port_manager
            .dequeue_key()
        {
            print!("{}", c as char);
        }
        if get_kernel_manager_cluster()
            .kernel_tty_manager
            .flush()
            .is_err()
        {
            pr_err!("Cannot flush text.");
        }
    }
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
            .memory_manager
            .lock()
            .unwrap()
            .free(VAddress::new(address))
        {
            pr_err!("Freeing the bitmap data of BGRT was failed: {:?}", e);
        }
    };
    let acpi_manager = get_kernel_manager_cluster().acpi_manager.lock().unwrap();

    let bgrt_manager = acpi_manager.get_xsdt_manager().get_bgrt_manager();
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
    let result = get_kernel_manager_cluster()
        .memory_manager
        .lock()
        .unwrap()
        .mmap_dev(
            boot_logo_physical_address.unwrap(),
            original_map_size,
            MemoryPermissionFlags::rodata(),
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

    let result = get_kernel_manager_cluster()
        .memory_manager
        .lock()
        .unwrap()
        .mremap_dev(
            boot_logo_address.into(),
            original_map_size,
            ((aligned_bitmap_width * bitmap_height as usize * (bitmap_color_depth as usize >> 3))
                + file_offset as usize)
                .into(),
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
    get_kernel_manager_cluster().serial_port_manager =
        SerialPortManager::new(0x3F8 /* COM1 */);
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

#[no_mangle]
pub extern "C" fn ap_boot_main() -> ! {
    /* Extern Assembly Symbols */
    extern "C" {
        pub static gdt: u64; /* boot/common.s */
        pub static tss_descriptor_address: u64; /* boot/common.s */
    }
    unsafe {
        cpu::enable_sse();
        cpu::enable_fs_gs_base();
    }

    /* Apply kernel paging table */
    get_kernel_manager_cluster()
        .memory_manager
        .lock()
        .unwrap()
        .set_paging_table();

    /* Setup CPU Manager, it contains individual data of CPU */
    let cpu_manager = setup_cpu_manager_cluster(None);
    let mut cpu_manager_list = &mut get_kernel_manager_cluster().boot_strap_cpu_manager.list;
    loop {
        if cpu_manager_list.get_next_as_ptr().is_none() {
            cpu_manager_list.insert_after(&mut cpu_manager.list);
            break;
        }
        cpu_manager_list = &mut unsafe { cpu_manager_list.get_next_mut() }.unwrap().list;
    }

    /* Setup memory management system */
    let mut object_allocator = ObjectAllocator::new();
    object_allocator.init(&mut get_kernel_manager_cluster().memory_manager.lock().unwrap());
    cpu_manager.object_allocator = Mutex::new(object_allocator);

    /* Copy GDT from BSP and create own TSS */
    let gdt_address = unsafe { &gdt as *const _ as usize };
    GateDescriptor::fork_gdt_from_other_and_create_tss_and_set(gdt_address, unsafe {
        &tss_descriptor_address as *const _ as usize - gdt_address
    } as u16);

    /* Setup InterruptManager(including LocalApicManager) */
    let mut interrupt_manager = InterruptManager::new();
    interrupt_manager.init_ap(
        &mut get_kernel_manager_cluster()
            .boot_strap_cpu_manager
            .interrupt_manager
            .lock()
            .unwrap(),
    );
    cpu_manager.cpu_id = interrupt_manager.get_local_apic_manager().get_apic_id() as usize;
    cpu_manager.interrupt_manager = Mutex::new(interrupt_manager);

    cpu_manager.arch_depend_data.local_apic_timer = init_timer();
    init_task_ap(ap_idle);
    init_interrupt_work_queue_manager();
    /* Switch to ap_idle task with own stack */
    cpu_manager.run_queue_manager.start()
}

fn ap_idle() -> ! {
    /* Tell BSP completing of init */
    init::AP_BOOT_COMPLETE_FLAG.store(true, core::sync::atomic::Ordering::Relaxed);
    /*get_cpu_manager_cluster()
    .arch_depend_data
    .local_apic_timer
    .start_interruption(
        get_cpu_manager_cluster()
            .interrupt_manager
            .lock()
            .unwrap()
            .get_local_apic_manager(),
    );*/
    /* For debug, suspend task_switch temporary */
    loop {
        unsafe {
            cpu::idle();
        }
    }
}
