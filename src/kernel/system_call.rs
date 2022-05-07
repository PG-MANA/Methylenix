//!
//! System Call Handler
//!

mod system_call_number;

use system_call_number::*;

use crate::arch::target_arch::context::context_data::ContextData;
use crate::arch::target_arch::device::cpu;
use crate::arch::target_arch::system_call;

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
                    context.set_system_call_return_value(
                        context.get_system_call_arguments(3).unwrap(),
                    );
                } else {
                    context.set_system_call_return_value(u64::MAX);
                }
            } else {
                pr_debug!(
                    "Unknown file descriptor: {}",
                    context.get_system_call_arguments(1).unwrap()
                );
                context.set_system_call_return_value(u64::MAX);
            }
        }
        SYSCALL_WRITEV => {
            if context.get_system_call_arguments(1).unwrap() == 1 {
                let mut written_bytes = 0usize;
                let iov = context.get_system_call_arguments(2).unwrap() as usize;
                for i in 0..(context.get_system_call_arguments(3).unwrap() as usize) {
                    use core::{mem, slice, str};
                    let iovec = iov + i * (mem::size_of::<usize>() * 2);
                    let iov_base = unsafe { *(iovec as *const usize) } as *const u8;
                    let iov_len = unsafe { *((iovec + mem::size_of::<usize>()) as *const usize) };
                    if let Ok(s) =
                        unsafe { str::from_utf8(slice::from_raw_parts(iov_base, iov_len)) }
                    {
                        kprint!("{s}");
                        written_bytes += iov_len;
                    } else {
                        break;
                    }
                }
                context.set_system_call_return_value(written_bytes as u64);
            } else {
                pr_debug!(
                    "Unknown file descriptor: {}",
                    context.get_system_call_arguments(1).unwrap()
                );
                context.set_system_call_return_value(u64::MAX);
            }
        }
        SYSCALL_ARCH_PRCTL => {
            let v = system_call::syscall_arch_prctl(context);
            context.set_system_call_return_value(v as u64);
        }
        s => {
            pr_err!("SysCall: Unknown({:#X})", s);
            context.set_system_call_return_value(u64::MAX);
        }
    }
}
