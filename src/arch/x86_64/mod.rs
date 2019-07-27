#[macro_use]
pub mod interrupt;
pub mod device;
pub mod paging;

use self::device::serial_port::SerialPortManager;
use self::device::{cpu, keyboard};

use kernel::drivers::efi::EfiManager;
use kernel::drivers::multiboot::MultiBootInformation;
use kernel::graphic::GraphicManager;
use kernel::memory_manager::MemoryManager;
use kernel::spin_lock::Mutex;
use kernel::struct_manager::STATIC_BOOT_INFORMATION_MANAGER;

#[no_mangle]
pub extern "C" fn boot_main(
    mbi_addr: usize, /*マルチブートヘッダのアドレス*/
    gdt: u64, /*現在のセグメント:8*/
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
    let mut memory_manager = MemoryManager::new(&multiboot_information);
    //IDT初期化&割り込み初期化
    let interrupt_manager = unsafe {
        interrupt::InterruptManager::new(memory_manager.alloc_page().expect("Cannot alloc memory for IDT."), gdt)
    };
    //シリアルポート初期化
    let serial_port_manager = SerialPortManager::new(0x3F8 /*COM1*/);
    serial_port_manager.init_serial_port(&interrupt_manager, gdt);

    if multiboot_information.efi_table_pointer != 0 {
        //EFI Bootが有効
        unsafe {
            STATIC_BOOT_INFORMATION_MANAGER.efi_manager =
                Mutex::new(EfiManager::new(multiboot_information.efi_table_pointer));
        }
    }
    //Boot Information Manager に格納
    unsafe {
        STATIC_BOOT_INFORMATION_MANAGER.memory_manager = Mutex::new(memory_manager);
        STATIC_BOOT_INFORMATION_MANAGER.interrupt_manager = Mutex::new(interrupt_manager);
        STATIC_BOOT_INFORMATION_MANAGER.serial_port_manager = Mutex::new(serial_port_manager);
    }
    println!("Methylenix version 0.0.1");

    unsafe {
        //IDT&PICの初期化が終わったのでSTIする
        cpu::sti();

        device::keyboard::Keyboard::init(
            &STATIC_BOOT_INFORMATION_MANAGER
                .interrupt_manager
                .lock()
                .unwrap(),
            gdt,
        );
    }
    hlt();
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
