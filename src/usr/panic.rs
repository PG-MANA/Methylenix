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
use core::fmt;

//Rust用失敗処理
#[lang = "eh_personality"]
extern "C" fn eh_personality() {}

#[lang = "panic_fmt"]
#[no_mangle]
pub extern "C" fn panic_fmt(args: fmt::Arguments, file: &str, line: u32) -> ! {
    println!("\n-- Kernel panic  -- You must restart this computer.\n-- Debug info --\nLine {} in {}\nMessage: {}\n-- End of the debug info --\nSystem will be halt.", line, &file, args);
    loop {
        unsafe {
            cpu::hlt();
        }
    }
}
