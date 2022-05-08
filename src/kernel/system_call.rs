//!
//! System Call Handler
//!

mod system_call_number;

use system_call_number::*;

use crate::arch::target_arch::context::context_data::ContextData;
use crate::arch::target_arch::device::cpu;
use crate::arch::target_arch::interrupt::InterruptManager;
use crate::arch::target_arch::system_call;

use crate::kernel::file_manager::File;
use crate::kernel::manager_cluster::get_cpu_manager_cluster;
use crate::kernel::memory_manager::data_type::VAddress;

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
        SYSCALL_EXIT_GROUP => {
            pr_info!(
                "SysCall: ExitGroup(Return Code: {:#X})",
                context.get_system_call_arguments(1).unwrap()
            );
            pr_info!("This thread will be stopped.");
            loop {
                unsafe { cpu::halt() };
            }
        }
        SYSCALL_WRITE => {
            let flag = InterruptManager::save_and_disable_local_irq();
            let process = get_cpu_manager_cluster()
                .run_queue
                .get_running_thread()
                .get_process_mut();
            InterruptManager::restore_local_irq(flag);

            let file = process.get_file(context.get_system_call_arguments(1).unwrap() as usize);
            if file.is_none() {
                pr_debug!(
                    "Unknown file descriptor: {}",
                    context.get_system_call_arguments(1).unwrap()
                );
                context.set_system_call_return_value(u64::MAX);
                return;
            }
            let result = system_call_write(
                &mut file.unwrap().lock().unwrap(),
                context.get_system_call_arguments(2).unwrap() as usize,
                context.get_system_call_arguments(3).unwrap() as usize,
            );
            context.set_system_call_return_value(
                result.and_then(|r| Ok(r as u64)).unwrap_or(u64::MAX),
            );
        }
        SYSCALL_WRITEV => {
            let flag = InterruptManager::save_and_disable_local_irq();
            let process = get_cpu_manager_cluster()
                .run_queue
                .get_running_thread()
                .get_process_mut();
            InterruptManager::restore_local_irq(flag);

            let file = process.get_file(context.get_system_call_arguments(1).unwrap() as usize);
            if file.is_none() {
                pr_debug!(
                    "Unknown file descriptor: {}",
                    context.get_system_call_arguments(1).unwrap()
                );
                context.set_system_call_return_value(u64::MAX);
                return;
            }
            let file = file.unwrap();
            let mut file_unlocked = file.lock().unwrap();
            let mut written_bytes = 0usize;
            let iov = context.get_system_call_arguments(2).unwrap() as usize;
            for i in 0..(context.get_system_call_arguments(3).unwrap() as usize) {
                use core::mem;
                let iovec = iov + i * (mem::size_of::<usize>() * 2);
                let iov_base = unsafe { *(iovec as *const usize) };
                let iov_len = unsafe { *((iovec + mem::size_of::<usize>()) as *const usize) };
                if let Ok(bytes) = system_call_write(&mut file_unlocked, iov_base, iov_len) {
                    written_bytes += bytes;
                } else {
                    break;
                }
            }
            drop(file);
            context.set_system_call_return_value(written_bytes as u64);
        }
        SYSCALL_ARCH_PRCTL => {
            let v = system_call::syscall_arch_prctl(context);
            context.set_system_call_return_value(v as u64);
        }
        SYSCALL_SET_TID_ADDRESS => {
            pr_debug!(
                "Ignore set_tid_address(address: {:#X})",
                context.get_system_call_arguments(1).unwrap()
            );
            let flag = InterruptManager::save_and_disable_local_irq();
            context.set_system_call_return_value(
                get_cpu_manager_cluster()
                    .run_queue
                    .get_running_thread()
                    .get_t_id() as u64,
            );
            InterruptManager::restore_local_irq(flag);
        }
        s => {
            pr_err!("SysCall: Unknown({:#X})", s);
            context.set_system_call_return_value(u64::MAX);
        }
    }
}

fn system_call_write(file: &mut File, data: usize, len: usize) -> Result<usize, ()> {
    //TODO: check address for security
    if data == 0 {
        return if len == 0 { Ok(0) } else { Err(()) };
    } else if len == 0 {
        return Ok(0);
    }
    file.write(VAddress::new(data), len)
}
