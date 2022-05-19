#![no_std]
#![no_main]
#![feature(abi_efiapi)]
#![feature(alloc_error_handler)]
#![feature(asm_const)]
#![feature(asm_sym)]
#![feature(const_for)]
#![feature(const_maybe_uninit_uninit_array)]
#![feature(const_mut_refs)]
#![feature(const_ptr_offset_from)]
#![feature(const_refs_to_cell)]
#![feature(const_size_of_val)]
#![feature(const_trait_impl)]
#![feature(format_args_nl)]
#![feature(let_else)]
#![feature(maybe_uninit_array_assume_init)]
#![feature(maybe_uninit_uninit_array)]
#![feature(naked_functions)]
#![feature(panic_info_message)]
#![feature(raw_ref_op)]
#![feature(step_trait)]
#![feature(try_blocks)]
#![feature(type_name_of_val)]

#[macro_use]
extern crate alloc;

pub const OS_NAME: &str = "Methylenix";
pub const OS_VERSION: &str = env!("CARGO_PKG_VERSION");

/* Arch independent modules */
#[macro_use]
pub mod kernel;

/* Arch-depend modules */
pub mod arch;
