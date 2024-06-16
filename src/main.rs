#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![feature(asm_const)]
#![feature(const_mut_refs)]
#![feature(const_size_of_val)]
#![feature(const_trait_impl)]
#![feature(effects)]
#![feature(linked_list_cursors)]
#![feature(maybe_uninit_uninit_array)]
#![feature(maybe_uninit_array_assume_init)]
#![feature(naked_functions)]
#![feature(panic_info_message)]
#![feature(step_trait)]
#![feature(strict_provenance)]
#![feature(try_blocks)]

#[macro_use]
extern crate alloc;

pub const OS_NAME: &str = "Methylenix";
pub const OS_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Arch independent modules
#[macro_use]
pub mod kernel;

/// Arch depended modules
pub mod arch;
