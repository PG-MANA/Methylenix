//!
//! Arch Depend System Call Handler
//!

pub mod system_call_number;

use system_call_number::*;

use crate::arch::target_arch::context::context_data::ContextData;
use crate::arch::target_arch::device::cpu;

use crate::kernel::system_call::ErrorCode;

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

pub fn arch_system_call_handler(s: SysCallNumber, context_data: &mut ContextData) -> bool {
    match s {
        SYSCALL_STAT => {
            pr_info!("stat(2) is not supported");
            context_data.set_system_call_return_value(ErrorCode::Invalid as _);
            true
        }
        _ => false,
    }
}
