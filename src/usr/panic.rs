/*
 * Copyright 2017 PG_MANA
 *
 * This software is Licensed under the Apache License Version 2.0
 * See LICENSE.md
 *
 *パニック時の処理を担当
 */

//use
use arch::target_arch::device::cpu;
use core::panic;

//Rust用失敗処理
#[lang = "eh_personality"]
extern "C" fn eh_personality() {}

#[panic_implementation]
#[no_mangle]
pub fn panic(info: &panic::PanicInfo) -> ! {
    let location = info.location();
    let message = info.message();

    println!("\n-- Kernel panic  -- You must restart this computer.\n-- Debug info --");
    if location.is_some() && message.is_some() {
        println!(
            "Line {} in {}\nMessage: {}",
            location.unwrap().line(),
            location.unwrap().file(),
            message.unwrap()
        );
    } else {
        println!("Not provided.");
    }
    println!("-- End of the debug info --\nSystem will be halt.");

    loop {
        unsafe {
            cpu::hlt();
        }
    }
}
