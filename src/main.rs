#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![feature(asm_const)]
#![feature(const_for)]
#![feature(const_maybe_uninit_uninit_array)]
#![feature(const_mut_refs)]
#![feature(const_refs_to_cell)]
#![feature(const_size_of_val)]
#![feature(const_trait_impl, effects)]
#![feature(maybe_uninit_array_assume_init)]
#![feature(maybe_uninit_uninit_array)]
#![feature(naked_functions)]
#![feature(panic_info_message)]
#![feature(ptr_metadata)]
#![feature(raw_ref_op)]
#![feature(step_trait)]
#![feature(try_blocks)]
#![feature(linked_list_cursors)]
#![feature(derive_const)]
#![feature(const_cmp)]
#![feature(const_ops)]

#[macro_use]
extern crate alloc;

pub const OS_NAME: &str = "Methylenix";
pub const OS_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Arch independent modules
#[macro_use]
pub mod kernel;

/// Arch depended modules
pub mod arch;
