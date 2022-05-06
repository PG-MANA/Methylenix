//!
//! Arch Depend System Call Handler
//!

use crate::arch::target_arch::context::context_data::ContextData;
use crate::arch::target_arch::device::cpu;

pub fn syscall_arch_prctl(context_data: &mut ContextData) -> u64 {
    const ARCH_SET_FS: u64 = 0x1002;
    match context_data.get_system_call_arguments(1).unwrap() {
        ARCH_SET_FS => {
            unsafe { cpu::set_fs_base(context_data.get_system_call_arguments(2).unwrap()) };
            0
        }
        _ => u64::MAX,
    }
}
