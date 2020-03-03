#[macro_use]
pub mod interrupt;
pub mod device;
pub mod paging;

use self::device::serial_port::SerialPortManager;
use self::device::cpu;
use self::interrupt::InterruptManager;
use self::device::io_apic::IoApicManager;
use self::device::local_apic::LocalApicManager;
use self::paging::{PageManager, PAGE_SIZE};

use kernel::drivers::multiboot::MultiBootInformation;
use kernel::graphic::GraphicManager;
use kernel::memory_manager::{MemoryManager, MemoryPermissionFlags};
use kernel::memory_manager::physical_memory_manager::PhysicalMemoryManager;
use kernel::memory_manager::virtual_memory_manager::VirtualMemoryManager;
use kernel::memory_manager::kernel_malloc_manager::KernelMemoryAllocManager;
use kernel::sync::spin_lock::Mutex;
use kernel::manager_cluster::get_kernel_manager_cluster;

use core::mem;

/* Memory Areas for initial processes*/
static mut MEMORY_FOR_PHYSICAL_MEMORY_MANAGER: [u8; PAGE_SIZE * 2] = [0; PAGE_SIZE * 2];


#[no_mangle]
pub extern "C" fn boot_main(
    mbi_address: usize, /*マルチブートヘッダのアドレス*/
    kernel_code_segment: u16, /*現在のセグメント:8*/
    user_code_segment: u16,
    user_data_segment: u16,
) {
    //この関数はCLIされた状態で呼ばれる。
    //PIC初期化
    device::pic::pic_init();
    //MultiBootInformation読み込み
    let multiboot_information = MultiBootInformation::new(mbi_address, true);
    // Graphic初期化（Panicが起きたときの表示のため)
    get_kernel_manager_cluster().graphic_manager =
        Mutex::new(GraphicManager::new(&multiboot_information.framebuffer_info));
    //メモリ管理初期化
    let _multiboot_information = init_memory(multiboot_information);
    //IDT初期化&割り込み初期化
    init_interrupt(kernel_code_segment);
    //シリアルポート初期化
    let serial_port_manager = SerialPortManager::new(0x3F8 /*COM1*/);
    serial_port_manager.init(kernel_code_segment);
    //Boot Information Manager に格納
    get_kernel_manager_cluster().serial_port_manager = Mutex::new(serial_port_manager);
    println!("Methylenix");

    let local_apic_manager = LocalApicManager::init();
    let io_apic_manager = IoApicManager::new();
    io_apic_manager.set_redirect(local_apic_manager.get_apic_id(), 4, 0x24);//Serial Port

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
    println!("All init are done!");
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
    let mut max_address = 0usize;
    let mut processed_address = 0usize;

    //set up for Physical Memory Manager
    let mut physical_memory_manager = PhysicalMemoryManager::new();
    unsafe {
        physical_memory_manager.set_memory_entry_pool((&MEMORY_FOR_PHYSICAL_MEMORY_MANAGER as *const _ as usize) as usize,
                                                      mem::size_of_val(&MEMORY_FOR_PHYSICAL_MEMORY_MANAGER));
    }
    for entry in multiboot_information.memory_map_info.clone() {
        if entry.m_type == 1 { //Free Area
            physical_memory_manager.define_free_memory(entry.addr as usize, entry.length as usize);
        }
        if (entry.addr + entry.length) as usize > max_address {
            max_address = (entry.addr + entry.length) as usize;
        }
    }

    //set up for Virtual Memory Manager
    let mut page_manager = PageManager::new(&mut physical_memory_manager).expect("Can not reset paging.");
    let mut virtual_memory_manager = VirtualMemoryManager::new();
    virtual_memory_manager.init(true, page_manager, &mut physical_memory_manager);

    //set up for Memory Manager
    let mut memory_manager = MemoryManager::new(Mutex::new(physical_memory_manager), virtual_memory_manager);

    for section in multiboot_information.elf_info.clone() {
        let permission = MemoryPermissionFlags {
            read: true,
            write: section.should_writable(),
            execute: section.should_excusable(),
            user_access: false,
        };
        memory_manager.reserve_pages(section.addr(), section.addr(), VirtualMemoryManager::size_to_order(section.size()), permission);
    }

    for entry in multiboot_information.memory_map_info.clone() {
        if entry.m_type == 1 {
            continue;
        }
        let permission = match entry.m_type {
            3 => MemoryPermissionFlags::data(),
            4 => MemoryPermissionFlags::data(),
            5 => MemoryPermissionFlags::data(), //rodata?
            _ => MemoryPermissionFlags::rodata()
        };
        memory_manager.reserve_pages(entry.addr as usize, entry.addr as usize, VirtualMemoryManager::size_to_order(entry.length as usize), permission);
    }

    // set up for Kernel Memory Alloc Manager
    let mut kernel_memory_alloc_manager = KernelMemoryAllocManager::new();
    kernel_memory_alloc_manager.init(&mut memory_manager);

    // Move multiboot information to allocated memory area.
    let new_mbi_address = kernel_memory_alloc_manager.kmalloc(multiboot_information.size, &mut memory_manager)
        .expect("Cannot alloc memory for Multiboot Information.");
    for i in 0..multiboot_information.size {
        unsafe {
            *((new_mbi_address + i) as *mut u8) = *((multiboot_information.address + i) as *mut u8);
        }
    }
    //Apply paging
    //memory_manager.reset_paging();

    //Store to cluster
    get_kernel_manager_cluster().memory_manager = Mutex::new(memory_manager);
    get_kernel_manager_cluster().kernel_memory_alloc_manager = Mutex::new(kernel_memory_alloc_manager);

    MultiBootInformation::new(new_mbi_address, false)
}


fn init_interrupt(kernel_selector: u16) {
    let mut interrupt_manager = InterruptManager::new();
    interrupt_manager.init(kernel_selector);
    get_kernel_manager_cluster().interrupt_manager = Mutex::new(interrupt_manager);
}