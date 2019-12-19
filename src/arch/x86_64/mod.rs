#[macro_use]
pub mod interrupt;
pub mod device;
pub mod paging;

use self::device::serial_port::SerialPortManager;
use self::device::{cpu, timer};
use self::device::cpu::get_func_addr;
use self::device::io_apic::IoApicManager;
use self::device::local_apic::LocalApicManager;
use self::paging::{PageManager, PAGE_MASK, PAGE_SIZE};

use kernel::drivers::efi::EfiManager;
use kernel::drivers::multiboot::MultiBootInformation;
use kernel::graphic::GraphicManager;
use kernel::memory_manager::MemoryManager;
use kernel::memory_manager::physical_memory_manager;
use kernel::spin_lock::Mutex;
use kernel::struct_manager::STATIC_BOOT_INFORMATION_MANAGER;
use kernel::memory_manager::physical_memory_manager::PhysicalMemoryManager;

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
    let multiboot_information = MultiBootInformation::new(mbi_addr);
    // Graphic初期化（Panicが起きたときの表示のため)
    unsafe {
        STATIC_BOOT_INFORMATION_MANAGER.graphic_manager =
            Mutex::new(GraphicManager::new(&multiboot_information.framebuffer_info));
    }
    //メモリ管理初期化
    init_memory(&multiboot_information);
    //IDT初期化&割り込み初期化
    let interrupt_manager = unsafe {
        let mut memory_manager = STATIC_BOOT_INFORMATION_MANAGER.memory_manager.lock().unwrap();
        interrupt::InterruptManager::new(memory_manager.alloc_page(None, false, true, false).expect("Cannot alloc memory for IDT."), kernel_code_segment)
    };
    /*unsafe {
                make_error_interrupt_hundler!(inthandler0d,general_protection_exception_handler);
                interrupt_manager.set_gatedec(
                    0x0d,
                    interrupt::idt::GateDescriptor::new(
                        inthandler0d, /*上のマクロで指定した名前*/
                        gdt as u16,
                        0,
                        interrupt::idt::GateDescriptor::AR_INTGATE32,
                    ),
                );
    }*/
    //シリアルポート初期化
    let serial_port_manager = SerialPortManager::new(0x3F8 /*COM1*/);
    serial_port_manager.init_serial_port(&interrupt_manager, kernel_code_segment);
    //Boot Information Manager に格納
    unsafe {
        STATIC_BOOT_INFORMATION_MANAGER.interrupt_manager = Mutex::new(interrupt_manager);
        STATIC_BOOT_INFORMATION_MANAGER.serial_port_manager = Mutex::new(serial_port_manager);
    }
    println!("Methylenix version 0.0.1");

    let local_apic_manager = LocalApicManager::init();
    let io_apic_manager = IoApicManager::new();
    io_apic_manager.set_redirect(local_apic_manager.get_apic_id(), 4, 0x24);//Serial Port

    unsafe {
        //IDT&PICの初期化が終わったのでSTIする
        cpu::sti();
    }
    //ページング反映
    unsafe {
        STATIC_BOOT_INFORMATION_MANAGER.task_manager.lock().unwrap().get_running_task_page_manager().reset_paging();
    }
    hlt();
}

pub fn general_protection_exception_handler(e_code: usize) {
    panic!("General Protection Exception \nError Code:0x{:X}", e_code);
}

fn hlt() {
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


fn init_memory(multiboot_information: &MultiBootInformation) {
    let mut max_address = 0usize;
    let mut processed_address = 0usize;
    //let mut memory_manager = MemoryManager::new(multiboot_information);
    //set up for Physical Memory Manager
    let mut physical_memory_manager = PhysicalMemoryManager::new();
    unsafe {
        physical_memory_manager.set_memory_entry_pool(&MEMORY_FOR_PHYSICAL_MEMORY_MANAGER as usize,
                                                      mem::size_of_val(MEMORY_FOR_PHYSICAL_MEMORY_MANAGER));
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

    unsafe {
        STATIC_BOOT_INFORMATION_MANAGER.memory_manager = Mutex::new(MemoryManager::new(Mutex::new(physical_memory_manager), page_manager));
    }
}
