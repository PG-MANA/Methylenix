#![no_std]
#![feature(asm)]
#![feature(const_fn)]
#![feature(const_mut_refs)]
#![feature(const_raw_ptr_deref)]
#![feature(lang_items)]
#![feature(naked_functions)]
#![feature(maybe_uninit_extra)]
#![feature(maybe_uninit_ref)]
#![feature(panic_info_message)]

//usr
#[macro_use]
pub mod kernel;

//arch
pub mod arch;

//そう...何もない!!
//各モジュールを参照してください。
