#![no_std]
#![feature(alloc_error_handler)]
#![feature(asm_const)]
#![feature(asm_sym)]
#![feature(cfg_target_has_atomic)]
#![feature(const_fn_fn_ptr_basics)]
#![feature(const_fn_trait_bound)]
#![feature(const_for)]
#![feature(const_mut_refs)]
#![feature(const_ptr_offset_from)]
#![feature(const_refs_to_cell)]
#![feature(const_size_of_val)]
#![feature(const_trait_impl)]
#![feature(lang_items)]
#![feature(maybe_uninit_array_assume_init)]
#![feature(maybe_uninit_extra)]
#![feature(maybe_uninit_uninit_array)]
#![feature(naked_functions)]
#![feature(panic_info_message)]
#![feature(raw_ref_op)]
#![feature(step_trait)]
#![feature(try_blocks)]
#![feature(type_name_of_val)]

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
