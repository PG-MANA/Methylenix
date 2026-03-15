//!
//! Arch Depend System Call Handler
//!

pub mod system_call_number;

use system_call_number::*;

use crate::arch::target_arch::context::context_data::ContextData;

pub fn syscall_arch_prctl(_: &mut ContextData) -> u64 {
    u64::MAX
}

pub fn arch_system_call_handler(_s: SysCallNumber, _context_data: &mut ContextData) -> bool {
    false
}
