//!
//! Arch Depend System Call Handler
//!

use crate::arch::target_arch::context::context_data::ContextData;

pub fn syscall_arch_prctl(_: &mut ContextData) -> u64 {
    u64::MAX
}
