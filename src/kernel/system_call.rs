//!
//! System Call Handler
//!

use crate::arch::target_arch::device::cpu;

use crate::arch::target_arch::context::context_data::ContextData;

pub fn system_call_handler(context: &mut ContextData) {
    match context.get_system_call_arguments(0).unwrap() {
        0x01 => {
            pr_info!(
                "SysCall: Exit(Return Code: {:#X})",
                context.get_system_call_arguments(1).unwrap()
            );
            pr_info!("This thread will be stopped.");
            loop {
                unsafe { cpu::halt() };
            }
        }
        0x04 => {
            if context.get_system_call_arguments(1).unwrap() == 1 {
                kprint!(
                    "{}",
                    unsafe {
                        core::str::from_utf8(core::slice::from_raw_parts(
                            context.get_system_call_arguments(2).unwrap() as *const u8,
                            context.get_system_call_arguments(3).unwrap() as usize,
                        ))
                    }
                    .unwrap_or("N/１A")
                );
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
