#![no_std]
#![feature(asm)]
#![feature(lang_items)]
#![feature(const_fn)]
#![feature(naked_functions)]
#![feature(core_panic_info)]
#![feature(panic_info_message)]

//usr
#[macro_use]
pub mod usr;

//arch
pub mod arch;



//そう...何もない!!
//各モジュールを参照してください。
