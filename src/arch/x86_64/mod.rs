#[macro_use]
pub mod interrupt;
pub mod device;
pub mod paging;

use self::device::serial_port::SerialPortManager;
use self::device::{cpu, keyboard, timer};
use self::paging::{PageManager, PAGE_MASK, PAGE_SIZE};

use kernel::drivers::efi::EfiManager;
use kernel::drivers::multiboot::MultiBootInformation;
use kernel::graphic::GraphicManager;
use kernel::memory_manager::MemoryManager;
use kernel::spin_lock::Mutex;
use kernel::struct_manager::STATIC_BOOT_INFORMATION_MANAGER;
use kernel::task::{TaskEntry, TaskStatus, TaskManager};
use self::device::cpu::get_func_addr;


//use x86_64::structures::idt::ExceptionStackFrame;

#[no_mangle]
pub extern "C" fn boot_main(
    mbi_addr: usize, /*マルチブートヘッダのアドレス*/
    kernel_code_segment: u16, /*現在のセグメント:8*/
    user_code_segment: u16,
    user_data_segment: u16,
) {
    //この関数はCLIされた状態で呼ばれる。
    //PIC初期化
    unsafe {
        device::pic::pic_init();
    }
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
        let page_manager = STATIC_BOOT_INFORMATION_MANAGER.task_manager.lock().unwrap().get_running_task_page_manager();
        interrupt::InterruptManager::new(page_manager.alloc_page(&mut memory_manager, None, None, false, true, false).expect("Cannot alloc memory for IDT."), kernel_code_segment)
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
    if multiboot_information.efi_table_pointer != 0 {
        //EFI Bootが有効
        unsafe {
            STATIC_BOOT_INFORMATION_MANAGER.efi_manager =
                Mutex::new(EfiManager::new(multiboot_information.efi_table_pointer));
        }
    }
    //Boot Information Manager に格納
    unsafe {
        STATIC_BOOT_INFORMATION_MANAGER.interrupt_manager = Mutex::new(interrupt_manager);
        STATIC_BOOT_INFORMATION_MANAGER.serial_port_manager = Mutex::new(serial_port_manager);
    }
    println!("Methylenix version 0.0.1");
    unsafe {
        //IDT&PICの初期化が終わったのでSTIする
        cpu::sti();
        /*
                device::keyboard::Keyboard::init(
                    &STATIC_BOOT_INFORMATION_MANAGER
                        .interrupt_manager
                        .lock()
                        .unwrap(),
                    gdt,
                );*/
    }
    //ページング反映
    unsafe {
        STATIC_BOOT_INFORMATION_MANAGER.task_manager.lock().unwrap().get_running_task_page_manager().reset_paging();
    }
    task_switch_test(user_code_segment, user_data_segment);
    timer::PitManager::init();
    hlt();
}

pub fn general_protection_exception_handler(e_code: usize) {
    panic!("General Protection Exception \nError Code:0x{:X}", e_code);
}

fn task_switch_test(user_code_segment: u16, user_data_segment: u16) {
    let mut task_manager = unsafe { STATIC_BOOT_INFORMATION_MANAGER.task_manager.lock().unwrap() };
    let mut memory_manager = unsafe { STATIC_BOOT_INFORMATION_MANAGER.memory_manager.lock().unwrap() };
    let mut task_entry = TaskEntry::new_static();
    let mut page_manager = task_manager.get_running_task_page_manager().clone();

    let task_switch_stack = page_manager.alloc_page(&mut memory_manager, None, None, false, true, false).expect("Can not alloc kernel stack.");
    let normal_stack = page_manager.alloc_page(&mut memory_manager, None, None, false, true, true).expect("Can not alloc normal stack.");
    unsafe {
        cpu::clear_task_stack(task_switch_stack, PAGE_SIZE, user_data_segment | 3, user_code_segment
            | 3, normal_stack + PAGE_SIZE - 16, cpu::get_func_addr(test_task));
        page_manager.associate_address(&mut memory_manager, None, get_func_addr(test_task) & PAGE_MASK, get_func_addr(test_task) & PAGE_MASK, true, false, true);
    }
    task_entry.set_kernel_stack(task_switch_stack);
    task_entry.set_status(TaskStatus::Running);
    task_entry.set_privilege_level(3);
    task_entry.set_enabled();
    task_entry.set_page_manager(page_manager);
    task_manager.add_task(task_entry);
}

pub fn test_task() {
    loop {
        //Loopするだけのタスク
    }
}

fn hlt() {
    const KEYCODE_MAP: [u8; 0x3a] = [
        b'0', b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'0', b'-', b'=',
        b'\x08', b'\t', b'q', b'w', b'e', b'r', b't', b'y', b'u', b'i', b'o', b'p', b'[', b']',
        b'\n', b'0', b'a', b's', b'd', b'f', b'g', b'h', b'j', b'k', b'l', b';', b'\'', b'`', b'0',
        b'\\', b'z', b'x', b'c', b'v', b'b', b'n', b'm', b',', b'.', b'/', b'0', b'*', b'0', b' ',
    ];
    print!("keyboard test:/ $");
    loop {
        unsafe {
            cpu::hlt();
        }
        let key_code = keyboard::Keyboard::dequeue_key().unwrap_or(0xff) as usize;
        if key_code < KEYCODE_MAP.len() {
            print!("{}", KEYCODE_MAP[key_code] as char);
        } else {
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
}


fn init_memory(multiboot_information: &MultiBootInformation) {
    //基本はストレートマッピングで行く
    //TODO: メモリマネージャーの4KAlign
    let mut memory_manager = MemoryManager::new(multiboot_information);
    let mut page_manager = PageManager::new(&mut memory_manager, None).expect("Can not reset paging.");
    let mut max_address = 0usize;
    //ページングを設定すべきメモリ範囲を出す
    for entry in multiboot_information.memory_map_info.clone() {
        if (entry.addr + entry.length) as usize > max_address {
            max_address = (entry.addr + entry.length) as usize;
        }
    }
    let mut counter = 0usize;
    loop {
        if max_address <= counter {
            break;
        }
        page_manager.associate_address(&mut memory_manager, None, counter, counter, false, false, false);
        counter += PAGE_SIZE;
    }
    for entry in multiboot_information.elf_info.clone() {
        counter = 0usize;
        let start_address = entry.addr() as usize & PAGE_MASK;
        loop {
            if entry.size() <= counter {
                break;
            }
            page_manager.associate_address(&mut memory_manager, None, start_address + counter,
                                           start_address + counter, entry.should_excusable(),
                                           entry.should_writable(), false);
            counter += PAGE_SIZE;
        }
    }
    //メモリマネージャーのPOOL用
    let memory_manager_pool = memory_manager.get_memory_pool();
    for i in 0..(memory_manager.get_memory_pool().1) / PAGE_SIZE {
        page_manager.associate_address(&mut memory_manager, None, memory_manager_pool.0 + i * PAGE_SIZE, memory_manager_pool.0 + i * PAGE_SIZE, false, true, false);
    }
    let mut task_manager = unsafe { STATIC_BOOT_INFORMATION_MANAGER.task_manager.lock().unwrap() };
    task_manager.set_entry_pool(page_manager.alloc_page(&mut memory_manager, None, None, false, true, false).expect("Can not alloc task manager pool."), PAGE_SIZE);
    let mut task_entry = TaskEntry::new_static();
    task_entry.set_status(TaskStatus::Running);
    task_entry.set_privilege_level(0);
    task_entry.set_kernel_stack(page_manager.alloc_page(&mut memory_manager, None, None, false, true, false).expect("Can not alloc kernel stack."));
    task_entry.set_enabled();
    task_entry.set_page_manager(page_manager);
    task_manager.add_task(task_entry);
    task_manager.switch_next_task(false);
    unsafe { STATIC_BOOT_INFORMATION_MANAGER.memory_manager = Mutex::new(memory_manager); }
}