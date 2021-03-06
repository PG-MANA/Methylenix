#![no_std]
#![feature(alloc_error_handler)]
#![feature(asm)]
#![feature(const_fn)]
#![feature(const_fn_fn_ptr_basics)]
#![feature(const_fn_trait_bound)]
#![feature(const_maybe_uninit_as_ptr)]
#![feature(const_mut_refs)]
#![feature(const_panic)]
#![feature(const_ptr_offset_from)]
#![feature(const_raw_ptr_deref)]
#![feature(const_refs_to_cell)]
#![feature(const_trait_impl)]
#![feature(global_asm)]
#![feature(lang_items)]
#![feature(maybe_uninit_array_assume_init)]
#![feature(maybe_uninit_extra)]
#![feature(maybe_uninit_ref)]
#![feature(maybe_uninit_uninit_array)]
#![feature(naked_functions)]
#![feature(panic_info_message)]
#![feature(raw_ref_op)]
#![feature(step_trait)]
#![feature(step_trait_ext)]
#![feature(try_blocks)]

#[allow(unused_imports)]
#[macro_use]
extern crate alloc;

pub const OS_NAME: &str = "Methylenix";
pub const OS_VERSION: &str = "0.0.1";

/* Arch independent modules */
#[macro_use]
pub mod kernel;

/* Arch-depend modules */
pub mod arch;
