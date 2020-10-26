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
use self::device::local_apic::LocalApicManager;
use self::device::local_apic_timer::LocalApicTimer;
use self::device::serial_port::SerialPortManager;
use self::init::multiboot::{init_graphic, init_memory_by_multiboot_information};
use self::init::{
    init_acpi, init_interrupt, init_interrupt_work_queue_manager, init_multiple_processors_ap,
    init_task, init_timer,
};

use crate::kernel::drivers::acpi::AcpiManager;
use crate::kernel::drivers::multiboot::MultiBootInformation;
use crate::kernel::graphic_manager::GraphicManager;
use crate::kernel::manager_cluster::{get_kernel_manager_cluster, CpuManagerCluster};
use crate::kernel::memory_manager::data_type::{Address, MSize};

use crate::kernel::memory_manager::MemoryPermissionFlags;
use crate::kernel::tty::TtyManager;

static mut LOCAL_APIC_TIMER: LocalApicTimer = LocalApicTimer::new();
static mut ACPI_MANAGER: Option<AcpiManager> = None;

pub struct ArchDependedCpuManagerCluster {
    pub self_pointer: usize,
}

#[no_mangle]
pub extern "C" fn multiboot_main(
    mbi_address: usize,       /* MultiBoot Information */
    kernel_code_segment: u16, /* Current segment is 8 */
    user_code_segment: u16,
    user_data_segment: u16,
) -> ! {
    unsafe { cpu::enable_sse() };
    get_kernel_manager_cluster().serial_port_manager =
        SerialPortManager::new(0x3F8 /* COM1 */); /* For debug */

    /* Load the multiboot information */
    let multiboot_information = MultiBootInformation::new(mbi_address, true);

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

    kprintln!("Methylenix");
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

    /* Init interruption */
    init_interrupt(kernel_code_segment);

    /* Setup Serial Port */
    get_kernel_manager_cluster().serial_port_manager.init();

    /* Setup ACPI */
    if let Some(rsdp_address) = multiboot_information.new_acpi_rsdp_ptr {
        unsafe { ACPI_MANAGER = init_acpi(rsdp_address) };
    } else {
        if multiboot_information.old_acpi_rsdp_ptr.is_some() {
            pr_warn!("ACPI 1.0 is not supported.");
        } else {
            pr_warn!("ACPI is not available.");
        }
    }

    /* Init Local APIC Timer*/
    unsafe { LOCAL_APIC_TIMER = init_timer(ACPI_MANAGER.as_ref()) };

    /* Set up graphic */
    init_graphic(&multiboot_information);

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
    if let Some(acpi) = unsafe { ACPI_MANAGER.as_ref() } {
        init_multiple_processors_ap(acpi);
    }

    /* Switch to main process */
    get_kernel_manager_cluster()
        .task_manager
        .execute_kernel_process()
    /* Never return to here */
}

pub fn general_protection_exception_handler(e_code: usize) {
    panic!("General Protection Exception \nError Code:0x{:X}", e_code);
}

fn main_process() -> ! {
    /* Interrupt is enabled */
    unsafe {
        LOCAL_APIC_TIMER.start_interruption(
            get_kernel_manager_cluster()
                .interrupt_manager
                .lock()
                .unwrap()
                .get_local_apic_manager(),
        );
    }
    pr_info!("All init are done!");

    /* Draw boot logo */
    if unsafe { ACPI_MANAGER.is_some() } {
        draw_boot_logo(unsafe { ACPI_MANAGER.as_ref().unwrap() });
    }

    kprintln!("Methylenix");
    loop {
        get_kernel_manager_cluster().task_manager.sleep();
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
            cpu::halt();
        }
    }
}

fn draw_boot_logo(acpi_manager: &AcpiManager) {
    let free_mapped_address = |address: usize| {
        if let Err(e) = get_kernel_manager_cluster()
            .memory_manager
            .lock()
            .unwrap()
            .free(address.into())
        {
            pr_err!("Freeing the bitmap data of BGRT was failed: {:?}", e);
        }
    };

    let boot_logo_physical_address = acpi_manager
        .get_xsdt_manager()
        .get_bgrt_manager()
        .get_bitmap_physical_address();
    let boot_logo_offset = acpi_manager
        .get_xsdt_manager()
        .get_bgrt_manager()
        .get_image_offset();
    if boot_logo_physical_address.is_none() || boot_logo_offset.is_none() {
        pr_info!("ACPI does not have the BGRT information.");
        return;
    }

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
            ((aligned_bitmap_width * bitmap_height as usize * (bitmap_color_depth as usize / 8))
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

    pr_info!(
        "BGRT: {:#X} is remapped at {:#X}",
        boot_logo_address,
        result.unwrap().to_usize(),
    );
    let boot_logo_address = result.unwrap();

    get_kernel_manager_cluster().graphic_manager.write_bitmap(
        (boot_logo_address + MSize::from(file_offset as usize)).into(),
        bitmap_color_depth as u8,
        bitmap_width as usize,
        bitmap_height as usize,
        boot_logo_offset.unwrap().0,
        boot_logo_offset.unwrap().1,
    );

    free_mapped_address(boot_logo_address.to_usize());
}

#[no_mangle]
pub extern "C" fn directboot_main(
    _info_address: usize,      /* DirectBoot Start Information */
    _kernel_code_segment: u16, /* Current segment is 8 */
    _user_code_segment: u16,
    _user_data_segment: u16,
) {
    get_kernel_manager_cluster()
        .serial_port_manager
        .sendstr("boot success\r\n");
    loop {
        unsafe {
            cpu::hlt();
        }
    }
}

#[no_mangle]
pub extern "C" fn unknown_boot_main() {
    SerialPortManager::new(0x3F8).sendstr("Unknown Boot System!");
    loop {
        unsafe { cpu::halt() };
    }
}

#[no_mangle]
pub extern "C" fn ap_boot_main() {
    unsafe { cpu::enable_sse() };
    get_kernel_manager_cluster()
        .memory_manager
        .lock()
        .unwrap()
        .set_paging_table();
    let mut local_apic_manager = LocalApicManager::new();
    local_apic_manager.init_from_other_manager(
        get_kernel_manager_cluster()
            .interrupt_manager
            .lock()
            .unwrap()
            .get_local_apic_manager(),
    );
    let cpu_manager_address = get_kernel_manager_cluster()
        .object_allocator
        .lock()
        .unwrap()
        .alloc(
            core::mem::size_of::<CpuManagerCluster>().into(),
            &get_kernel_manager_cluster().memory_manager,
        )
        .unwrap();
    /*  "mov rax, gs:0" is same as "let rax = *(gs as *const u64)".
        we cannot load gs.base by "lea rax, [gs:0]" because lea cannot use gs register in x86_64.
        On general kernel, the per-CPU's data struct has a member pointing itself and accesses it.
    */

    let cpu_manager = unsafe { &mut *(cpu_manager_address.to_usize() as *mut CpuManagerCluster) };
    cpu_manager.arch_depend_data.self_pointer = cpu_manager_address.to_usize(); /* self pointer */

    unsafe {
        cpu::set_gs_and_kernel_gs_base(
            &cpu_manager.arch_depend_data.self_pointer as *const _ as u64,
        )
    };

    cpu_manager.cpu_id = local_apic_manager.get_apic_id() as usize;

    init::AP_BOOT_COMPLETE_FLAG.store(true, core::sync::atomic::Ordering::Relaxed);

    loop {
        unsafe { cpu::halt() };
    }
}
