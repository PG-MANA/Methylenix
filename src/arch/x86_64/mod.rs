#[macro_use]
pub mod interrupt;
pub mod device;
pub mod paging;

use self::device::serial_port::SerialPortManager;
use self::device::cpu;
use self::interrupt::InterruptManager;
use self::device::io_apic::IoApicManager;
use self::device::local_apic::LocalApicManager;
use self::paging::{PageManager, PAGE_MASK, PAGE_SIZE};

use kernel::drivers::multiboot::MultiBootInformation;
use kernel::graphic::GraphicManager;
use kernel::memory_manager::MemoryManager;
use kernel::memory_manager::physical_memory_manager::PhysicalMemoryManager;
use kernel::spin_lock::Mutex;
use kernel::struct_manager::STATIC_BOOT_INFORMATION_MANAGER;
use kernel::kernel_malloc::KernelMemoryAllocManager;

use core::mem;

/* Memory Areas for initial processes*/
static mut MEMORY_FOR_PHYSICAL_MEMORY_MANAGER: [u8; PAGE_SIZE * 2] = [0; PAGE_SIZE * 2];


#[no_mangle]
pub extern "C" fn boot_main(
    mbi_addr: usize, /*マルチブートヘッダのアドレス*/
    kernel_code_segment: u16, /*現在のセグメント:8*/
    user_code_segment: u16,
    user_data_segment: u16,
) {
    //この関数はCLIされた状態で呼ばれる。
    //PIC初期化
    device::pic::pic_init();
    //MultiBootInformation読み込み
    let multiboot_information = MultiBootInformation::new(mbi_addr, true);
    // Graphic初期化（Panicが起きたときの表示のため)
    unsafe {
        STATIC_BOOT_INFORMATION_MANAGER.graphic_manager =
            Mutex::new(GraphicManager::new(&multiboot_information.framebuffer_info));
    }
    //メモリ管理初期化
    let _multiboot_information = init_memory(multiboot_information);
    //IDT初期化&割り込み初期化
    init_interrupt(kernel_code_segment);
    //シリアルポート初期化
    let serial_port_manager = SerialPortManager::new(0x3F8 /*COM1*/);
    serial_port_manager.init(kernel_code_segment);
    //Boot Information Manager に格納
    unsafe {
        STATIC_BOOT_INFORMATION_MANAGER.serial_port_manager = Mutex::new(serial_port_manager);
    }
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
        let ascii_code = unsafe {
            STATIC_BOOT_INFORMATION_MANAGER
                .serial_port_manager
                .lock()
                .unwrap()
                .dequeue_key()
                .unwrap_or(0)
        };
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
    for section in multiboot_information.elf_info.clone() {
        physical_memory_manager.define_used_memory(section.addr(), section.size());
    }
    physical_memory_manager.define_used_memory(multiboot_information.address, multiboot_information.size);

    //set up for Page Manager (Virtual Memory Manager)
    let mut page_manager = PageManager::new(&mut physical_memory_manager).expect("Can not reset paging.");
    while processed_address < max_address {
        page_manager.associate_address(&mut physical_memory_manager, processed_address, processed_address, false, false, false);
        processed_address += PAGE_SIZE;
    }
    for entry in multiboot_information.elf_info.clone() {
        processed_address = entry.addr() as usize & PAGE_MASK;
        while processed_address < (entry.size() + entry.addr()) {
            page_manager.associate_address(&mut physical_memory_manager, processed_address,
                                           processed_address, entry.should_excusable(),
                                           entry.should_writable(), false);
            processed_address += PAGE_SIZE;
        }
    }


    //set up for Memory Manager
    let mut memory_manager = MemoryManager::new(Mutex::new(physical_memory_manager), page_manager);

    // set up for Kernel Memory Alloc Manager
    let mut kernel_memory_alloc_manager = KernelMemoryAllocManager::new();
    kernel_memory_alloc_manager.init(&mut memory_manager);

    // Move multiboot information to allocated memory area.
    let new_mbi_address = kernel_memory_alloc_manager.malloc(&mut memory_manager, multiboot_information.size)
        .expect("Cannot alloc memory for Multiboot Information.");
    for i in 0..multiboot_information.size {
        unsafe {
            *((new_mbi_address + i) as *mut u8) = *((multiboot_information.address + i) as *mut u8);
        }
    }
    memory_manager.free_physical_memory(multiboot_information.address, multiboot_information.size);

    //Apply paging
    memory_manager.reset_paging();

    unsafe {
        STATIC_BOOT_INFORMATION_MANAGER.memory_manager = Mutex::new(memory_manager);
        STATIC_BOOT_INFORMATION_MANAGER.kernel_memory_alloc_manager = Mutex::new(kernel_memory_alloc_manager);
    }
    MultiBootInformation::new(new_mbi_address, false)
}


fn init_interrupt(kernel_selector: u16) {
    let mut interrupt_manager = InterruptManager::new();
    interrupt_manager.init(kernel_selector);
    unsafe {
        STATIC_BOOT_INFORMATION_MANAGER.interrupt_manager = Mutex::new(interrupt_manager);
    }
}