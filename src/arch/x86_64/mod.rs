/*
 * Copyright 2017 PG_MANA
 *
 * This software is Licensed under the Apache License Version 2.0
 * See LICENSE.md
 *
 * grub.asmから呼ばれる何か。
 * Rust入門コード
 * C言語との連携はないと考えている...考えているつもり
 */

#[macro_use]
pub mod vga;
pub mod mbi; //Multi Boot Information
pub mod memman;
//mod paging;
#[macro_use]
pub mod interrupt;
pub mod device;

//use
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
    puts!("Methylenix ver.0.0.1\n");
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
    let mut memory_manager = memman_init(&info, addr);
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

    //memman::init(&info.meminfo);
    //memman::set_elf(info.elfinfo);
    //puts!("MemMan will have been initialized...perhaps...\n");
    //println!("Alloc the {}th page,{}th page,{}th page",memman::alloc4k(),memman::alloc4k(),memman::alloc4k());
    //memman::show();
    hlt();
}

fn memman_init(info: &MultiBootInformation, mbiaddr: usize) -> MemoryManager {
    //カーネルサイズの計算
    let mut elfinfo = info.elfinfo.clone();
    let kernel_loader_start = elfinfo.map(|section| section.addr()).min().unwrap();
    elfinfo = info.elfinfo.clone();
    let kernel_loader_end = elfinfo.map(|section| section.addr()).max().unwrap();
    let mbi_start = mbiaddr;
    let mbi_end = mbiaddr + mbi::total_size(mbiaddr) as usize;
    println!(
        "KernelLoader Size:{}KB,MultiBootInformation Size:{}KB",
        (kernel_loader_end - kernel_loader_start) / 1024 as usize,
        mbi::total_size(mbiaddr) / 1024
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
        puts!("Hmm..Total_size is zero.\n");
    }
    let info = mbi::MultiBootInformation::new(addr); //Result型などがあり利用してみるのもいいかも
    info
}

fn hlt() {
    let mut cnt = 1;
    loop {
        unsafe {
            cpu::hlt();
        }
        print!(
            "interrupted {}th. Key code:{:02x} (print from Kernel))\r",
            cnt,
            keyboard::Keyboard::dequeue_key().unwrap()
        );
        cnt += 1;
    }
}
