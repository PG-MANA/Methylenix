//!
//! Arch-depended modules
//!

#[cfg(target_arch = "x86_64")]
pub mod x86_64;

#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64 as target_arch;
/* We can access target-specific struct as arch::target_arch */
