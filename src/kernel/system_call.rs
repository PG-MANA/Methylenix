//!
//! System Call Handler
//!

mod system_call_number;

use system_call_number::*;

use crate::arch::target_arch::context::context_data::ContextData;
use crate::arch::target_arch::device::cpu;

pub fn system_call_handler(context: &mut ContextData) {
    match context.get_system_call_arguments(0).unwrap() as SysCallNumber {
        SYSCALL_EXIT => {
            pr_info!(
                "SysCall: Exit(Return Code: {:#X})",
                context.get_system_call_arguments(1).unwrap()
            );
            pr_info!("This thread will be stopped.");
            loop {
                unsafe { cpu::halt() };
            }
        }
        SYSCALL_WRITE => {
            if context.get_system_call_arguments(1).unwrap() == 1 {
                if let Ok(s) = unsafe {
                    core::str::from_utf8(core::slice::from_raw_parts(
                        context.get_system_call_arguments(2).unwrap() as *const u8,
                        context.get_system_call_arguments(3).unwrap() as usize,
                    ))
                } {
                    kprint!("{s}");
                }
            } else {
                pr_debug!(
                    "Unknown file descriptor: {}",
                    context.get_system_call_arguments(1).unwrap()
                );
            }
        }
        s => {
            pr_err!("SysCall: Unknown({:#X})", s);
        }
    }
}
