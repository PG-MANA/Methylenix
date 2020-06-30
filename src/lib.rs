#![no_std]
#![feature(alloc_error_handler)]
#![feature(const_fn)]
#![feature(const_generics)]
#![feature(const_mut_refs)]
#![feature(const_panic)]
#![feature(const_raw_ptr_deref)]
#![feature(global_asm)]
#![feature(lang_items)]
#![feature(llvm_asm)]
#![feature(maybe_uninit_extra)]
#![feature(maybe_uninit_ref)]
#![feature(naked_functions)]
#![feature(panic_info_message)]
#![feature(track_caller)]

#[macro_use]
extern crate alloc;

//usr
#[macro_use]
pub mod kernel;

//arch
pub mod arch;
