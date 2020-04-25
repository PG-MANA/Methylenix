/*
 * x86_64 Boot Entry
 */

#[macro_use]
pub mod interrupt;
pub mod boot;
pub mod device;
pub mod paging;

use self::device::cpu;
use self::device::serial_port::SerialPortManager;
use self::interrupt::InterruptManager;
use self::paging::{PAGE_MASK, PAGE_SIZE};

use kernel::drivers::acpi::AcpiManager;
use kernel::drivers::multiboot::MultiBootInformation;
use kernel::graphic::GraphicManager;
use kernel::manager_cluster::get_kernel_manager_cluster;
use kernel::memory_manager::kernel_malloc_manager::KernelMemoryAllocManager;
use kernel::memory_manager::physical_memory_manager::PhysicalMemoryManager;
use kernel::memory_manager::virtual_memory_manager::VirtualMemoryManager;
use kernel::memory_manager::{MemoryManager, MemoryOptionFlags, MemoryPermissionFlags};
use kernel::sync::spin_lock::Mutex;

use core::mem;

/* Memory Areas for initial processes*/
static mut MEMORY_FOR_PHYSICAL_MEMORY_MANAGER: [u8; PAGE_SIZE * 2] = [0; PAGE_SIZE * 2];

#[no_mangle]
pub extern "C" fn boot_main(
    mbi_address: usize,       /* MultiBoot Information */
    kernel_code_segment: u16, /* Current segment is 8 */
    _user_code_segment: u16,
    _user_data_segment: u16,
) {
    /* MultiBootInformation読み込み */
    let multiboot_information = MultiBootInformation::new(mbi_address, true);
    /* Graphic初期化（Panicが起きたときの表示のため) */
    get_kernel_manager_cluster().graphic_manager =
        Mutex::new(GraphicManager::new(&multiboot_information.framebuffer_info));
    kprintln!("Methylenix");
    /* メモリ管理初期化 */
    let multiboot_information = init_memory(multiboot_information);
    if !get_kernel_manager_cluster()
        .graphic_manager
        .lock()
        .unwrap()
        .set_frame_buffer_memory_permission()
    {
        panic!("Cannot map memory for frame buffer");
    }
    /* IDT初期化&割り込み初期化 */
    init_interrupt(kernel_code_segment);
    /* シリアルポート初期化 */
    let serial_port_manager = SerialPortManager::new(0x3F8 /* COM1 */);
    serial_port_manager.init();
    /* Boot Information Manager に格納 */
    get_kernel_manager_cluster().serial_port_manager = Mutex::new(serial_port_manager);
    if let Some(rsdp_address) = multiboot_information.new_acpi_rsdp_ptr {
        init_acpi(rsdp_address);
    } else {
        if multiboot_information.old_acpi_rsdp_ptr.is_some() {
            pr_warn!("ACPI 1.0 is not supported.");
        } else {
            pr_warn!("ACPI is not available.");
        }
    }
    pr_info!(
        "Booted from {}, cmd line: {}",
        multiboot_information.boot_loader_name,
        multiboot_information.boot_cmd_line
    );
    unsafe {
        //IDT&PICの初期化が終わったのでSTIする
        cpu::sti();
    }
    hlt();
}

pub fn general_protection_exception_handler(e_code: usize) {
    panic!("General Protection Exception \nError Code:0x{:X}", e_code);
}

fn hlt() {
    pr_info!("All init are done!");
    loop {
        unsafe {
            cpu::hlt();
        }
        let ascii_code = get_kernel_manager_cluster()
            .serial_port_manager
            .lock()
            .unwrap()
            .dequeue_key()
            .unwrap_or(0);
        if ascii_code != 0 {
            print!("{}", ascii_code as char);
        }
    }
}

fn init_memory(multiboot_information: MultiBootInformation) -> MultiBootInformation {
    /* set up Physical Memory Manager */
    let mut physical_memory_manager = PhysicalMemoryManager::new();
    unsafe {
        physical_memory_manager.set_memory_entry_pool(
            &MEMORY_FOR_PHYSICAL_MEMORY_MANAGER as *const _ as usize,
            mem::size_of_val(&MEMORY_FOR_PHYSICAL_MEMORY_MANAGER),
        );
    }
    for entry in multiboot_information.memory_map_info.clone() {
        if entry.m_type == 1 {
            /* available memory */
            physical_memory_manager.free(entry.addr as usize, entry.length as usize, true);
        }
    }
    /* 先に使用中のメモリ領域を除外するためelfセクションを解析 */
    for section in multiboot_information.elf_info.clone() {
        if section.should_allocate() && section.align_size() == PAGE_SIZE {
            physical_memory_manager.reserve_memory(section.addr(), section.size(), true);
        }
    }
    /* reserve Multiboot Information area */
    physical_memory_manager.reserve_memory(
        multiboot_information.address,
        multiboot_information.size,
        false,
    );

    /* set up Virtual Memory Manager */
    let mut virtual_memory_manager = VirtualMemoryManager::new();
    if !virtual_memory_manager.init(true, &mut physical_memory_manager) {
        panic!("Cannot init Virtual Memory Manager.");
    }

    for section in multiboot_information.elf_info.clone() {
        if !section.should_allocate() || section.align_size() != PAGE_SIZE {
            continue;
        }
        let permission = MemoryPermissionFlags::new(
            true,
            section.should_writable(),
            section.should_excusable(),
            false,
        );
        let aligned_start_address = section.addr() & PAGE_MASK;
        let aligned_size = ((section.size() + (section.addr() - aligned_start_address) - 1)
            & PAGE_MASK)
            + PAGE_SIZE;
        /* 初期化の段階で1 << order 分のメモリ管理を行ってはいけない。他の領域と重なる可能性がある。*/
        if let Ok(address) = virtual_memory_manager.map_address(
            aligned_start_address,
            Some(aligned_start_address),
            aligned_size,
            permission,
            MemoryOptionFlags::new(MemoryOptionFlags::NORMAL),
            &mut physical_memory_manager,
        ) {
            if address == aligned_start_address {
                continue;
            }
        }
        panic!("Cannot map virtual memory correctly.");
    }
    /* may be needless */
    if false {
        for entry in multiboot_information.memory_map_info.clone() {
            if entry.m_type == 1 {
                continue;
            }
            let permission = match entry.m_type {
                3 => MemoryPermissionFlags::data(), /* ACPI */
                4 => MemoryPermissionFlags::data(),
                5 => MemoryPermissionFlags::data(), //rodata?
                _ => MemoryPermissionFlags::rodata(),
            };
            let aligned_start_address = entry.addr as usize & PAGE_MASK;
            let aligned_size =
                ((entry.addr as usize - aligned_start_address + entry.length as usize - 1)
                    & PAGE_MASK)
                    + PAGE_SIZE;
            if let Ok(address) = virtual_memory_manager.map_address(
                aligned_start_address,
                Some(aligned_start_address),
                aligned_size,
                permission,
                MemoryOptionFlags::new(MemoryOptionFlags::NORMAL),
                &mut physical_memory_manager,
            ) {
                if address == aligned_start_address {
                    continue;
                }
            }
            panic!("Cannot map virtual memory correctly.");
        }
    }
    /* set up Memory Manager */
    let mut memory_manager =
        MemoryManager::new(Mutex::new(physical_memory_manager), virtual_memory_manager);

    /* set up Kernel Memory Alloc Manager */
    let mut kernel_memory_alloc_manager = KernelMemoryAllocManager::new();
    kernel_memory_alloc_manager.init(&mut memory_manager);

    /* move Multiboot Information to allocated memory area */
    let new_mbi_address = kernel_memory_alloc_manager
        .kmalloc(multiboot_information.size, &mut memory_manager)
        .expect("Cannot alloc memory for Multiboot Information.");
    for i in 0..multiboot_information.size {
        unsafe {
            *((new_mbi_address + i) as *mut u8) = *((multiboot_information.address + i) as *mut u8);
        }
    }

    /* free old multibootinfo area */
    memory_manager.free_physical_memory(multiboot_information.address, multiboot_information.size); /* may be already freed */
    /* apply paging */
    memory_manager.set_paging_table();

    /* store managers to cluster */
    get_kernel_manager_cluster().memory_manager = Mutex::new(memory_manager);
    get_kernel_manager_cluster().kernel_memory_alloc_manager =
        Mutex::new(kernel_memory_alloc_manager);
    MultiBootInformation::new(new_mbi_address, false)
}

fn init_interrupt(kernel_selector: u16) {
    device::pic::disable_8259_pic();
    let mut interrupt_manager = InterruptManager::new();
    interrupt_manager.init(kernel_selector);
    get_kernel_manager_cluster().interrupt_manager = Mutex::new(interrupt_manager);
}

fn init_acpi(rsdp_ptr: usize) {
    use core::str;

    let mut acpi_manager = AcpiManager::new();
    if !acpi_manager.init(rsdp_ptr) {
        pr_warn!("Cannot init ACPI.");
        return;
    }
    pr_info!(
        "OEM ID:{}",
        str::from_utf8(&acpi_manager.get_oem_id().unwrap_or([0; 6])).unwrap_or("NODATA")
    );

    if let Some(p_bitmap_address) = acpi_manager
        .get_xsdt_manager()
        .get_bgrt_manager()
        .get_bitmap_physical_address()
    {
        match get_kernel_manager_cluster()
            .memory_manager
            .lock()
            .unwrap()
            .memory_remap(
                p_bitmap_address,
                54,
                MemoryPermissionFlags::rodata(),
                MemoryOptionFlags::new(MemoryOptionFlags::NORMAL),
            ) {
            Ok(bitmap_vm_address) => {
                pr_info!(
                    "0x{:X} is mapped at 0x{:X}",
                    p_bitmap_address,
                    bitmap_vm_address
                );
                if !draw_boot_logo(
                    bitmap_vm_address,
                    acpi_manager
                        .get_xsdt_manager()
                        .get_bgrt_manager()
                        .get_image_offset()
                        .unwrap(),
                ) {
                    get_kernel_manager_cluster()
                        .memory_manager
                        .lock()
                        .unwrap()
                        .free_pages(bitmap_vm_address, 0 /*must fix*/);
                }
            }
            Err(err_massage) => {
                pr_err!("{}", err_massage);
            }
        };
    }
}

fn draw_boot_logo(bitmap_vm_address: usize, offset: (usize, usize)) -> bool {
    /* if file_size > PAGE_SIZE => ?*/
    if unsafe { *((bitmap_vm_address + 30) as *const u32) } != 0 {
        pr_info!("Boot logo is compressed");
        return false;
    }
    let file_offset = unsafe { *((bitmap_vm_address + 10) as *const u32) };
    let bitmap_width = unsafe { *((bitmap_vm_address + 18) as *const u32) };
    let bitmap_height = unsafe { *((bitmap_vm_address + 22) as *const u32) };
    let bitmap_color_depth = unsafe { *((bitmap_vm_address + 28) as *const u16) };
    let aligned_bitmap_width =
        ((bitmap_width as usize * (bitmap_color_depth as usize / 8) - 1) & !3) + 4;
    match get_kernel_manager_cluster()
        .memory_manager
        .lock()
        .unwrap()
        .resize_memory_remap(
            bitmap_vm_address,
            (aligned_bitmap_width * bitmap_height as usize * (bitmap_color_depth as usize / 8))
                + file_offset as usize,
        ) {
        Ok(remapped_bitmap_vm_address) => {
            pr_info!(
                "Bitmap Data: 0x{:X} is remapped at 0x{:X}",
                bitmap_vm_address,
                remapped_bitmap_vm_address,
            );
            get_kernel_manager_cluster()
                .graphic_manager
                .lock()
                .unwrap()
                .write_bitmap(
                    remapped_bitmap_vm_address + file_offset as usize,
                    bitmap_color_depth as u8,
                    bitmap_width as usize,
                    bitmap_height as usize,
                    offset.0,
                    offset.1,
                );
            get_kernel_manager_cluster()
                .memory_manager
                .lock()
                .unwrap()
                .free_pages(remapped_bitmap_vm_address, 0 /*must fix*/);
            return true;
        }
        Err(err_message) => {
            pr_err!("{}", err_message);
            return false;
        }
    };
}
