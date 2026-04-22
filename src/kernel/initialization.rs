//!
//! The functions for initialization
//!
//! This module contains initialization functions which is not depend on arch.
//!

use crate::arch::target_arch::context::memory_layout::physical_address_to_direct_map;
use crate::arch::target_arch::device::{cpu, pci::ArchDependPciManager};
use crate::arch::target_arch::paging::{PAGE_MASK, PAGE_SIZE_USIZE};

use crate::kernel::{
    application_loader,
    block_device::BlockDeviceManager,
    collections::init_struct,
    drivers::{
        acpi::{
            AcpiManager,
            device::AcpiDeviceManager,
            table::{bgrt::BgrtManager, mcfg::McfgManager},
        },
        boot_information::*,
        efi::{
            EFI_ACPI_2_0_TABLE_GUID, EFI_PAGE_SIZE, memory_map::EfiMemoryType,
            protocol::graphics_output_protocol::EfiGraphicsPixelFormat,
        },
        pci::PciManager,
    },
    file_manager::{FileManager, elf::ELF_PROGRAM_HEADER_SEGMENT_LOAD},
    graphic_manager::{GraphicManager, font::FontType},
    manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster},
    memory_manager::{
        MemoryManager,
        data_type::{Address, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress, VAddress},
        io_remap,
        memory_allocator::MemoryAllocator,
        mremap,
        physical_memory_manager::PhysicalMemoryManager,
        system_memory_manager::{SystemMemoryManager, get_physical_memory_manager},
        virtual_memory_manager::VirtualMemoryManager,
    },
    sync::spin_lock::Mutex,
    task_manager::run_queue::RunQueue,
    timer_manager::GlobalTimerManager,
};

/// Init application processor's TaskManager
///
///
pub fn init_task_ap(idle_task: fn() -> !) {
    let mut run_queue = RunQueue::new();
    run_queue.init().expect("Failed to init RunQueue");

    get_kernel_manager_cluster()
        .task_manager
        .init_idle(idle_task, &mut run_queue);
    get_cpu_manager_cluster().run_queue = run_queue;
}

/// Init Work Queue
pub fn init_work_queue() {
    get_cpu_manager_cluster()
        .work_queue
        .init_cpu_work_queue(&mut get_kernel_manager_cluster().task_manager);
}

/// Memory Areas for PhysicalMemoryManager
pub static mut MEMORY_FOR_PHYSICAL_MEMORY_MANAGER: [u8; PAGE_SIZE_USIZE * 2] =
    [0; PAGE_SIZE_USIZE * 2];

/// Init memory system based on [`BootInformation`].
/// This function sets up [`PhysicalMemoryManager`] which manages where is free
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

    /* Free usable memory area */
    for entry in boot_information.memory_map.iter() {
        if entry.memory_type == EfiMemoryType::MaxMemoryType {
            continue;
        }
        pr_info!(
            "[{:#016X}~{:#016X}] {}",
            entry.physical_start,
            MSize::new((entry.number_of_pages as usize) << 12)
                .to_end_address(PAddress::new(entry.physical_start))
                .to_usize(),
            entry.memory_type
        );
        match entry.memory_type {
            EfiMemoryType::ConventionalMemory
            | EfiMemoryType::BootServicesCode
            | EfiMemoryType::LoaderCode => {
                let start_address = PAddress::new(entry.physical_start);
                let size = MSize::new((entry.number_of_pages as usize) * EFI_PAGE_SIZE);
                bug_on_err!(physical_memory_manager.free(start_address, size, true));
                if start_address + size > max_usable_memory_address {
                    max_usable_memory_address = start_address + size;
                }
            }
            _ => {}
        }
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

    for entry in boot_information.elf_program_headers.iter() {
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
    }

    /* Set up Memory Manager */
    init_struct!(
        get_kernel_manager_cluster().kernel_memory_manager,
        MemoryManager::new(virtual_memory_manager)
    );

    /* Adjust Memory Pointer */
    /* `efi_memory_map_address` and `elf_program_headers_address` are already direct mapped. */
    if let Some(system_table) = &mut boot_information.efi_system_table {
        system_table.set_configuration_table(
            physical_address_to_direct_map(PAddress::new(system_table.get_configuration_table()))
                .to_usize(),
        );
    }
    /*boot_information.font_address = boot_information.font_address.map(|a| {
        (
            (physical_address_to_direct_map(PAddress::new(a.0)).to_usize()),
            a.1,
        )
    });*/

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

/// Init [`GraphicManager`] with [`BootInformation`]
///
/// This function will get framebuffer information and remap the address.
/// Do not call this before initializing memory manager.
///
/// If suitable framebuffer was found, this will process and return true.
/// Otherwise, this will do nothing and return false.
pub fn init_graphic_by_boot_information(boot_information: &BootInformation) -> bool {
    init_struct!(
        get_kernel_manager_cluster().graphic_manager,
        GraphicManager::default()
    );
    if let Some(graphic_info) = &boot_information.graphic_info {
        if graphic_info.info.pixel_format
            == EfiGraphicsPixelFormat::PixelBlueGreenRedReserved8BitPerColor
        {
            get_kernel_manager_cluster()
                .graphic_manager
                .set_frame_buffer(
                    graphic_info.frame_buffer_address,
                    graphic_info.info.horizontal_resolution as usize,
                    graphic_info.info.vertical_resolution as usize,
                    32,
                );
            get_kernel_manager_cluster()
                .graphic_manager
                .remap_frame_buffer()
                .expect("Failed to remap frame buffer");
            return true;
        } else {
            pr_warn!(
                "Unsupported pixel format: {:?}",
                graphic_info.info.pixel_format
            );
        }
    }
    false
}

/// Load font into [`GraphicManager`]
///
/// Currently, supports PFF2 font only.
pub fn init_graphic_font_by_boot_information(boot_information: &BootInformation) -> bool {
    let Some(font_info) = &boot_information.font_info else {
        return false;
    };
    /* TODO: reserve memory */
    get_kernel_manager_cluster().graphic_manager.load_font(
        font_info.font_address,
        MSize::new(font_info.font_size.get()),
        FontType::Pff2,
    )
}

/// Init AcpiManager without parsing AML
///
/// This function initializes ACPI Manager.
/// ACPI Manager will parse some tables and return.
/// If succeeded, this will move it into kernel_manager_cluster.
pub fn init_acpi_early(rsdp_ptr: usize) -> bool {
    let mut acpi_manager = AcpiManager::new();
    let mut device_manager = AcpiDeviceManager::new();
    let set_manger = |a: AcpiManager, d: AcpiDeviceManager| {
        init_struct!(get_kernel_manager_cluster().acpi_manager, Mutex::new(a));
        init_struct!(get_kernel_manager_cluster().acpi_device_manager, d);
    };

    if !acpi_manager.init(rsdp_ptr, &mut device_manager) {
        pr_warn!("Cannot init ACPI.");
        set_manger(acpi_manager, device_manager);
        return false;
    }
    if let Some(e) = acpi_manager.create_acpi_event_manager() {
        init_struct!(get_kernel_manager_cluster().acpi_event_manager, e);
    } else {
        pr_err!("Cannot init ACPI Event Manager");
        set_manger(acpi_manager, device_manager);
        return false;
    }
    set_manger(acpi_manager, device_manager);
    true
}

/// Init AcpiManager without parsing AML
///
/// This function get ACPI's RSDP Pointer from [`BootInformation::efi_system_table`]
/// and pass it to [`init_acpi_early`].
///
/// If the ACPI Table is not found or [`init_acpi_early`] returns false, this returns false.
/// For more information, please see [`init_acpi_early`].
pub fn init_acpi_early_by_boot_information(boot_information: &BootInformation) -> bool {
    boot_information
        .efi_system_table
        .as_ref()
        .and_then(|t| t.find_vendor_table(EFI_ACPI_2_0_TABLE_GUID))
        .map(|rsdp_ptr| init_acpi_early(rsdp_ptr))
        .unwrap_or(false)
}

/// Init AcpiManager and AcpiEventManager with parsing AML
///
/// This function will set up some devices like power button.
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
    if !crate::arch::target_arch::device::acpi::setup_interrupt(&acpi_manager) {
        pr_err!("Cannot setup ACPI interrupt.");
        return false;
    }
    if !acpi_manager.setup_acpi_devices(&mut get_kernel_manager_cluster().acpi_device_manager) {
        pr_err!("Cannot setup ACPI devices.");
        return false;
    }
    if !acpi_manager.initialize_all_devices() {
        pr_err!("Cannot evaluate _STA/_INI methods.");
        return false;
    }
    get_kernel_manager_cluster()
        .acpi_event_manager
        .init_event_registers();
    if !acpi_manager.enable_acpi() {
        pr_err!("Cannot enable ACPI.");
        return false;
    }
    if !acpi_manager.enable_power_button(&mut get_kernel_manager_cluster().acpi_event_manager) {
        pr_err!("Cannot enable power button.");
        return false;
    }
    get_kernel_manager_cluster()
        .acpi_event_manager
        .enable_gpes();
    true
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
            pci_manager = PciManager::new_arch_depend(ArchDependPciManager::new());
        }
    } else {
        pci_manager = PciManager::new_arch_depend(ArchDependPciManager::new());
    }
    init_struct!(get_kernel_manager_cluster().pci_manager, pci_manager);
    if let Err(err) = get_kernel_manager_cluster().pci_manager.build_device_tree() {
        pr_err!("Failed to build PCI device tree: {:?}", err);
        return false;
    }
    true
}

/// Init PciManager with scanning all bus
pub fn init_pci_later() -> bool {
    get_kernel_manager_cluster().pci_manager.setup_devices();
    true
}

/// Init global timer
pub fn init_global_timer() {
    init_struct!(
        get_kernel_manager_cluster().global_timer_manager,
        GlobalTimerManager::new()
    );
}

/// Initialize Block Device Manager and File System Manager
///
/// This function must be called before calling device scan functions.
pub fn init_block_devices_and_file_system_early() {
    init_struct!(
        get_kernel_manager_cluster().block_device_manager,
        BlockDeviceManager::new()
    );
    init_struct!(
        get_kernel_manager_cluster().file_manager,
        FileManager::new()
    );
}

/// Initialize Network Manager
///
/// This function must be called before calling device scan functions.
pub fn init_network_manager_early() {
    get_kernel_manager_cluster().network_manager.init();
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

/// Mount Root System
///
/// Currently, mount the first detected file system as root
/// TODO: support command line
pub fn mount_root_file_system() {
    if let Some(uuid) = get_kernel_manager_cluster().file_manager.get_first_uuid() {
        pr_info!("Mount {uuid} as root");
        get_kernel_manager_cluster()
            .file_manager
            .mount_root(uuid, true);
    } else {
        pr_info!("No root partition was found");
    }
}

/// Draw the OEM Logo by ACPI's BGRT
pub fn draw_boot_logo() {
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

    let original_map_size = MSize::new(54);
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

    use core::ptr::read_unaligned;
    if unsafe { read_unaligned((boot_logo_address + 30) as *const u32) } != 0 {
        pr_info!("Boot logo is compressed");
        free_mapped_address(boot_logo_address);
        return;
    }
    let file_offset = unsafe { read_unaligned((boot_logo_address + 10) as *const u32) };
    let bitmap_width = unsafe { read_unaligned((boot_logo_address + 18) as *const u32) };
    let bitmap_height = unsafe { read_unaligned((boot_logo_address + 22) as *const u32) };
    let bitmap_color_depth = unsafe { read_unaligned((boot_logo_address + 28) as *const u16) };
    let aligned_bitmap_width =
        ((bitmap_width as usize * (bitmap_color_depth as usize / 8) - 1) & !3) + 4;

    let result = mremap!(
        VAddress::new(boot_logo_address),
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

pub fn idle() -> ! {
    loop {
        unsafe {
            cpu::idle();
        }
    }
}

/// Main process called after finishing arch-depend initializations
pub fn main_initialization_process() -> ! {
    pr_info!("Entered main initialization process");

    draw_boot_logo();

    init_block_devices_and_file_system_early();
    init_network_manager_early();

    if !init_pci_early() {
        pr_err!("Cannot init PCI Manager.");
    }

    if !init_acpi_later() {
        pr_err!("Cannot init ACPI devices.");
    }

    if !init_pci_later() {
        pr_err!("Cannot init PCI devices.");
    }

    init_block_devices_and_file_system_later();

    mount_root_file_system();

    let _ = crate::kernel::network_manager::dhcp::get_ipv4_address_sync(0);

    pr_info!("Execute the init process");
    const ENVIRONMENT_VARIABLES: [(&str, &str); 3] = [
        ("OSTYPE", crate::OS_NAME),
        ("OSVERSION", crate::OS_VERSION),
        ("TARGET", crate::arch::target_arch::TARGET_ARCH_NAME),
    ];
    const INIT_PROCESS_PATH: &str = "/sbin/init";
    let _ = application_loader::load_and_execute(INIT_PROCESS_PATH, &[], &ENVIRONMENT_VARIABLES);

    idle()
}
