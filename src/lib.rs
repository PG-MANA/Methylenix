#![no_std]
#![feature(asm)]
#![feature(const_fn)]
#![feature(core_panic_info)]
#![feature(lang_items)]
#![feature(naked_functions)]
#![feature(panic_info_message)]
#![feature(const_raw_ptr_deref)]
#![feature(const_mut_refs)]
#![feature(maybe_uninit_extra)]
#![feature(maybe_uninit_ref)]

//usr
#[macro_use]
pub mod kernel;

//arch
pub mod arch;

//そう...何もない!!
//各モジュールを参照してください。
