#[cfg(target_arch = "x86_64")]
pub mod x86_64;

#[cfg(target_arch = "x86_64")]
pub use arch::x86_64 as target_arch;
/* we can access target-specific struct as arch::target_arch */
