#![no_std]
#![no_main]
#![feature(allocator_api)]
#![feature(array_try_from_fn)]
#![feature(const_ops)]
#![feature(const_convert)]
#![feature(const_trait_impl)]
#![feature(step_trait)]
#![feature(try_blocks)]
#[macro_use]
extern crate alloc;

pub const OS_NAME: &str = env!("CARGO_PKG_NAME");
pub const OS_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Arch independent modules
#[macro_use]
pub mod kernel;

/// Arch depended modules
pub mod arch;
