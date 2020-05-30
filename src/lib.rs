#![no_std]
#![feature(const_fn)]
#![feature(const_if_match)]
#![feature(const_loop)]
#![feature(const_mut_refs)]
#![feature(const_panic)]
#![feature(const_raw_ptr_deref)]
#![feature(global_asm)]
#![feature(lang_items)]
#![feature(llvm_asm)]
#![feature(maybe_uninit_extra)]
#![feature(maybe_uninit_ref)]
#![feature(naked_functions)]
#![feature(no_more_cas)]
#![feature(panic_info_message)]
#![feature(track_caller)]

//usr
#[macro_use]
pub mod kernel;

//arch
pub mod arch;
