#[macro_use]
pub mod vga;
pub mod mbi; //Multi Boot Information
pub mod memman;
//mod paging;
#[macro_use]
pub mod interrupt;
pub mod device;

use arch::target_arch::device::{cpu, keyboard};
use arch::target_arch::mbi::*;
use arch::target_arch::memman::MemoryManager;

#[allow(unused_imports)]
use core::fmt;

#[no_mangle]
pub extern "C" fn boot_main(
    addr: usize, /*マルチブートヘッダのアドレス*/
    gdt: u64,    /*現在のセグメント:8*/
) {
    //おそらくこの関数はCLIされた状態で呼ばれる。
    println!("Methylenix version 0.0.1");
    //PIC初期化
    unsafe {
        device::pic::pic_init();
    }

    //MultiBootInformation読み込み
    if !mbi::test(addr) {
        panic!("Unaligned Multi Boot Information.");
    }
    let info = load_mbi(addr);
    //メモリ管理初期化
    let mut memory_manager = init_memman(&info, addr);
    //IDT初期化&割り込み初期化
    let idt_manager =
        unsafe { interrupt::IDTMan::new(memory_manager.alloc_page().unwrap().get_page(), gdt) };
    unsafe {
        device::keyboard::Keyboard::init(&idt_manager, gdt);
    }
    //IDT&PICの初期化が終わったのでSTIする
    unsafe {
        cpu::sti();
    }
    if info.framebufferinfo.depth == 32 {
        // 文字見えてないだろうから#FF7F27で塗りつぶす
        for count in 0..(info.framebufferinfo.width * info.framebufferinfo.height) {
            unsafe {
                *((info.framebufferinfo.address + (count * 4) as u64) as *mut u32) = 0xff7f27;
            }
        }
    }
    hlt();
}

fn init_memman(info: &MultiBootInformation, mbiaddr: usize) -> MemoryManager {
    //カーネルサイズの計算
    let kernel_loader_start = info
        .elfinfo
        .clone()
        .map(|section| section.addr())
        .min()
        .unwrap();
    let kernel_loader_end = info
        .elfinfo
        .clone()
        .map(|section| section.addr())
        .max()
        .unwrap();
    let mbi_start = mbiaddr;
    let mbi_end = mbiaddr + mbi::total_size(mbiaddr) as usize;
    println!(
        "KernelLoader Size:{}KB, MultiBootInformation Size:{}B",
        (kernel_loader_end - kernel_loader_start) / 1024 as usize,
        mbi::total_size(mbiaddr)
    );
    memman::MemoryManager::new(
        info.memmapinfo.clone(),
        kernel_loader_start as usize,
        kernel_loader_end as usize,
        mbi_start,
        mbi_end,
    )
}

fn load_mbi(addr: usize) -> mbi::MultiBootInformation {
    let mbi_total_size = mbi::total_size(addr);
    if mbi_total_size == 0 {
        panic!("Invalid Multi Boot Information.");
    }
    let info = mbi::MultiBootInformation::new(addr); //Result型などがあり利用してみるのもいいかも
    info
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
        let keycode = keyboard::Keyboard::dequeue_key().unwrap() as usize;
        if keycode < KEYCODE_MAP.len() {
            print!("{}", KEYCODE_MAP[keycode] as char);
        }
    }
}
