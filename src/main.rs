#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![feature(const_ops)]
#![feature(const_trait_impl)]
#![feature(maybe_uninit_array_assume_init)]
#![feature(step_trait)]
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
