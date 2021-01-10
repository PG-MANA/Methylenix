#![no_std]
#![feature(alloc_error_handler)]
#![feature(const_fn)]
#![feature(const_fn_fn_ptr_basics)]
#![feature(const_mut_refs)]
#![feature(const_panic)]
#![feature(const_trait_impl)]
#![feature(global_asm)]
#![feature(lang_items)]
#![feature(asm)]
#![feature(maybe_uninit_extra)]
#![feature(maybe_uninit_ref)]
#![feature(naked_functions)]
#![feature(panic_info_message)]
#![feature(step_trait)]
#![feature(step_trait_ext)]

#[allow(unused_imports)]
#[macro_use]
extern crate alloc;

/* Arch independent modules */
#[macro_use]
pub mod kernel;

/* Arch-depend modules */
pub mod arch;
