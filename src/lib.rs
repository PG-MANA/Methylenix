#![no_std]
#![feature(asm)]
#![feature(lang_items)]
#![feature(const_fn)]
#![feature(naked_functions)]
#![feature(use_extern_macros)]
#![feature(panic_handler)]
#![feature(core_panic_info)]
#![feature(panic_info_message)]

//arch
#[macro_use]
pub mod arch;

//usr
pub mod usr;

//そう...何もない!!
//各モジュールを参照してください。
