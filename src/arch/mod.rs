//!
//! Arch-depended modules
//!

#[cfg(target_arch = "x86_64")]
pub mod x86_64;

#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64 as target_arch;

#[cfg(target_arch = "aarch64")]
pub mod aarch64;

#[cfg(target_arch = "aarch64")]
pub use crate::arch::aarch64 as target_arch;

#[cfg(target_arch = "riscv64")]
pub mod riscv64;

#[cfg(target_arch = "riscv64")]
pub use crate::arch::riscv64 as target_arch;

/* We can access target-specific struct as arch::target_arch */
