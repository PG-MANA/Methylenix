//!
//! System Call Handler
//!

mod system_call_number;

use system_call_number::*;

use crate::arch::target_arch::context::context_data::ContextData;
use crate::arch::target_arch::device::cpu;
use crate::arch::target_arch::interrupt::InterruptManager;
use crate::arch::target_arch::system_call;

use crate::kernel::file_manager::{File, FileSeekOrigin, PathInfo, FILE_PERMISSION_READ};
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::data_type::{
    Address, MOffset, MSize, MemoryOptionFlags, MemoryPermissionFlags, VAddress,
};
use crate::kernel::network_manager::socket_manager::socket_system_call;

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
            let process = get_cpu_manager_cluster().run_queue.get_running_process();

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
            let process = get_cpu_manager_cluster().run_queue.get_running_process();

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
            if written_bytes == 0 {
                context.set_system_call_return_value(u64::MAX);
            } else {
                context.set_system_call_return_value(written_bytes as u64);
            }
        }
        SYSCALL_READ => {
            let process = get_cpu_manager_cluster().run_queue.get_running_process();
            let file = process.get_file(context.get_system_call_arguments(1).unwrap() as usize);
            if file.is_none() {
                pr_debug!(
                    "Unknown file descriptor: {}",
                    context.get_system_call_arguments(1).unwrap()
                );
                context.set_system_call_return_value(u64::MAX);
                return;
            }
            let result = file.unwrap().lock().unwrap().read(
                VAddress::new(context.get_system_call_arguments(2).unwrap() as usize),
                MSize::new(context.get_system_call_arguments(3).unwrap() as usize),
            );
            context.set_system_call_return_value(
                result
                    .and_then(|r| Ok(r.to_usize() as u64))
                    .unwrap_or(u64::MAX),
            );
        }
        SYSCALL_OPEN => {
            const O_RDONLY: u64 = 0;
            const O_LARGEFILE: u64 = 00100000;

            let mut str_len = 0usize;
            let file_name = context.get_system_call_arguments(1).unwrap() as usize;
            while unsafe { *((file_name + str_len) as *const u8) } != 0 {
                str_len += 1;
            }
            let mut flag = context.get_system_call_arguments(2).unwrap();
            flag &= !O_LARGEFILE;
            if flag == O_RDONLY {
                if let Ok(s) = core::str::from_utf8(unsafe {
                    core::slice::from_raw_parts(file_name as *const u8, str_len)
                }) {
                    if let Ok(f) = get_kernel_manager_cluster()
                        .file_manager
                        .file_open(PathInfo::new(s), FILE_PERMISSION_READ)
                    {
                        let process = get_cpu_manager_cluster().run_queue.get_running_process();
                        let fd = process.add_file(f);
                        context.set_system_call_return_value(fd as u64);
                    } else {
                        pr_warn!("{} is not found.", s);
                    }
                } else {
                    pr_warn!("Failed to convert file name to utf-8");
                    context.set_system_call_return_value(u64::MAX);
                }
            } else {
                pr_warn!(
                    "Unsupported flags: {:#X}",
                    context.get_system_call_arguments(2).unwrap()
                );
                context.set_system_call_return_value(u64::MAX);
            }
        }
        SYSCALL_LSEEK => {
            const SEEK_SET: u64 = 0x00;
            const SEEK_CUR: u64 = 0x01;
            const SEEK_END: u64 = 0x02;
            let seek_origin = match context.get_system_call_arguments(3).unwrap() {
                SEEK_SET => FileSeekOrigin::SeekSet,
                SEEK_CUR => FileSeekOrigin::SeekCur,
                SEEK_END => FileSeekOrigin::SeekEnd,
                _ => {
                    pr_debug!(
                        "Invalid Seek Option: {:#X}",
                        context.get_system_call_arguments(3).unwrap()
                    );
                    context.set_system_call_return_value(u64::MAX);
                    return;
                }
            };

            let process = get_cpu_manager_cluster().run_queue.get_running_process();
            let file = process.get_file(context.get_system_call_arguments(1).unwrap() as usize);
            if file.is_none() {
                pr_debug!(
                    "Unknown file descriptor: {}",
                    context.get_system_call_arguments(1).unwrap()
                );
                context.set_system_call_return_value(u64::MAX);
                return;
            }

            let result = file.unwrap().lock().unwrap().seek(
                MOffset::new(context.get_system_call_arguments(2).unwrap() as usize),
                seek_origin,
            );
            context.set_system_call_return_value(
                result
                    .and_then(|r| Ok(r.to_usize() as u64))
                    .unwrap_or(u64::MAX),
            );
        }
        SYSCALL_CLOSE => {
            let process = get_cpu_manager_cluster().run_queue.get_running_process();
            let file = process.get_file(context.get_system_call_arguments(1).unwrap() as usize);
            if file.is_none() {
                pr_debug!(
                    "Unknown file descriptor: {}",
                    context.get_system_call_arguments(1).unwrap()
                );
                context.set_system_call_return_value(u64::MAX);
                return;
            }
            let file = unsafe {
                core::ptr::replace(&mut *file.unwrap().lock().unwrap(), File::new_invalid())
            };
            file.close();
            context.set_system_call_return_value(0);
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
        SYSCALL_BRK => {
            pr_debug!(
                "BRK(Address: {:#X}) is ignored.",
                context.get_system_call_arguments(1).unwrap()
            );
            context.set_system_call_return_value(u64::MAX);
        }
        SYSCALL_MMAP => {
            let address = context.get_system_call_arguments(1).unwrap();
            let size = context.get_system_call_arguments(2).unwrap();
            let prot = context.get_system_call_arguments(3).unwrap_or(0);
            let flags = context.get_system_call_arguments(4).unwrap_or(0);
            let fd = context.get_system_call_arguments(5).unwrap_or(0);
            let offset = context.get_system_call_arguments(6).unwrap_or(0);
            context.set_system_call_return_value(
                system_call_memory_map(
                    address as usize,
                    size as usize,
                    prot as usize,
                    flags as usize,
                    fd as usize,
                    offset as usize,
                )
                .unwrap_or(usize::MAX) as u64,
            );
        }
        SYSCALL_MUNMAP => {
            let address = context.get_system_call_arguments(1).unwrap();
            let memory_manager = unsafe {
                &mut *(get_cpu_manager_cluster()
                    .run_queue
                    .get_running_process()
                    .get_memory_manager())
            };
            let result = memory_manager.free(VAddress::new(address as usize));
            context.set_system_call_return_value(if let Err(e) = result {
                pr_err!("Failed to free memory: {:?}", e);
                u64::MAX
            } else {
                0
            });
        }
        SYSCALL_SOCKET => {
            let domain_number = context.get_system_call_arguments(1).unwrap();
            let socket_type_number = context.get_system_call_arguments(2).unwrap();
            let protocol_number = context.get_system_call_arguments(3).unwrap();
            let socket = socket_system_call::create_socket(
                domain_number,
                socket_type_number,
                protocol_number,
            );
            if let Err(err) = socket {
                pr_warn!("Failed to create socket: {:?}", err);
                context.set_system_call_return_value(u64::MAX);
                return;
            }
            let process = get_cpu_manager_cluster().run_queue.get_running_process();
            let fd = process.add_file(socket.unwrap());
            context.set_system_call_return_value(fd as u64);
        }
        SYSCALL_BIND => {
            let process = get_cpu_manager_cluster().run_queue.get_running_process();
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
            let sock_addr_address = context.get_system_call_arguments(2).unwrap();
            let sock_addr_size = context.get_system_call_arguments(3).unwrap();
            if sock_addr_size as usize != core::mem::size_of::<socket_system_call::SockAddr>() {
                pr_debug!("Unsupported the size of SockAddr: {sock_addr_size}");
                context.set_system_call_return_value(u64::MAX);
                return;
            }
            if let Err(err) = socket_system_call::bind_socket(&mut file.lock().unwrap(), unsafe {
                &*(sock_addr_address as usize as *const socket_system_call::SockAddr)
            }) {
                pr_err!("Failed to bind socket: {:?}", err);
                context.set_system_call_return_value(u64::MAX);
                return;
            }
            context.set_system_call_return_value(0);
        }
        SYSCALL_LISTEN => {
            let process = get_cpu_manager_cluster().run_queue.get_running_process();
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
            let max_connection = context.get_system_call_arguments(2).unwrap();
            if let Err(err) = socket_system_call::listen_socket(
                &mut file.lock().unwrap(),
                max_connection as usize,
            ) {
                pr_err!("Failed to listen socket: {:?}", err);
                context.set_system_call_return_value(u64::MAX);
                return;
            }
            context.set_system_call_return_value(0);
        }
        SYSCALL_ACCEPT => {
            let process = get_cpu_manager_cluster().run_queue.get_running_process();
            let file = process.get_file(context.get_system_call_arguments(1).unwrap() as usize);
            if file.is_none() {
                pr_debug!(
                    "Unknown file descriptor: {}",
                    context.get_system_call_arguments(1).unwrap()
                );
                context.set_system_call_return_value(u64::MAX);
                return;
            }
            //let sock_addr_address = context.get_system_call_arguments(2).unwrap();
            //let sock_addr_size_address = context.get_system_call_arguments(3).unwrap();
            /*if sock_addr_size as usize != core::mem::size_of::<socket_system_call::SockAddr>() {
                pr_debug!("Unsupported the size of SockAddr: {sock_addr_size}");
                context.set_system_call_return_value(u64::MAX);
                return;
            }*/
            let file = file.unwrap();
            let result = socket_system_call::accept(&mut file.lock().unwrap());
            if let Err(err) = result {
                pr_debug!("Failed to accept connection: {:?}", err);
                context.set_system_call_return_value(u64::MAX);
                return;
            }
            let (file, _sock_addr) = result.unwrap();
            let process = get_cpu_manager_cluster().run_queue.get_running_process();
            let fd = process.add_file(file);
            /*let _ = write_data_into_user(
                VAddress::new(sock_addr_address as usize),
                MSize::new(sock_addr_size as usize),
                VAddress::new(&sock_addr as *const _ as usize),
            );*/
            context.set_system_call_return_value(fd as u64);
        }
        SYSCALL_RECVFROM => {
            let process = get_cpu_manager_cluster().run_queue.get_running_process();
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
            let buffer_size = MSize::new(context.get_system_call_arguments(3).unwrap() as usize);
            let buffer_address = match check_user_address(
                context.get_system_call_arguments(2).unwrap() as usize,
                buffer_size,
                false,
                true,
            ) {
                Ok(a) => a,
                Err(_) => {
                    pr_warn!(
                        "Invalid user address: {:#X}",
                        context.get_system_call_arguments(2).unwrap()
                    );
                    context.set_system_call_return_value(u64::MAX);
                    return;
                }
            };
            //let sock_addr_address = context.get_system_call_arguments(5).unwrap();
            //let sock_addr_size_address = context.get_system_call_arguments(6).unwrap();

            match socket_system_call::recv_from(
                &mut file.lock().unwrap(),
                buffer_address,
                buffer_size,
                context.get_system_call_arguments(4).unwrap() as usize,
                None,
            ) {
                Ok(a) => {
                    context.set_system_call_return_value(a.to_usize() as u64);
                }
                Err(err) => {
                    pr_warn!("Failed to receive data: {:?}", err);
                    context.set_system_call_return_value(u64::MAX);
                }
            }
        }
        SYSCALL_SENDTO => {
            let process = get_cpu_manager_cluster().run_queue.get_running_process();
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
            let buffer_size = MSize::new(context.get_system_call_arguments(3).unwrap() as usize);
            let buffer_address = match check_user_address(
                context.get_system_call_arguments(2).unwrap() as usize,
                buffer_size,
                true,
                false,
            ) {
                Ok(a) => a,
                Err(_) => {
                    pr_err!(
                        "Invalid user address: {:#X}",
                        context.get_system_call_arguments(2).unwrap()
                    );
                    context.set_system_call_return_value(u64::MAX);
                    return;
                }
            };
            //let sock_addr_address = context.get_system_call_arguments(5).unwrap();
            //let sock_addr_size = context.get_system_call_arguments(6).unwrap();

            match socket_system_call::send_to(
                &mut file.lock().unwrap(),
                buffer_address,
                buffer_size,
                context.get_system_call_arguments(4).unwrap() as usize,
                None,
            ) {
                Ok(a) => {
                    context.set_system_call_return_value(a.to_usize() as u64);
                }
                Err(err) => {
                    pr_err!("Failed to send data: {:?}", err);
                    context.set_system_call_return_value(u64::MAX);
                }
            }
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
    file.write(VAddress::new(data), MSize::new(len))
        .and_then(|s| Ok(s.to_usize()))
}

fn system_call_memory_map(
    address: usize,
    size: usize,
    prot: usize,
    flags: usize,
    _fd: usize,
    _offset: usize,
) -> Result<usize, ()> {
    /* PROT */
    const PROT_NONE: usize = 0x00;
    const PROT_READ: usize = 0x01;
    const PROT_WRITE: usize = 0x02;
    const PROT_EXEC: usize = 0x04;

    /* FLAGS */
    //const MAP_SHARED: usize = 0x01;
    //const MAP_PRIVATE: usize = 0x02;
    //const MAP_FIXED: usize = 0x10;
    const MAP_ANONYMOUS: usize = 0x20;

    if size == 0 {
        pr_err!("Size is zero.");
        return Err(());
    }
    let size = MSize::new(size).page_align_up();

    let memory_permission = MemoryPermissionFlags::new(
        (prot & PROT_READ) != 0,
        (prot & PROT_WRITE) != 0,
        (prot & PROT_EXEC) != 0,
        prot != PROT_NONE,
    );
    if (flags & MAP_ANONYMOUS) == 0 {
        pr_err!("Flags({:#X}) is not anonymous.", flags);
        return Err(());
    }
    let memory_options = MemoryOptionFlags::ALLOC | MemoryOptionFlags::USER;

    let memory_manager = unsafe {
        &mut *(get_cpu_manager_cluster()
            .run_queue
            .get_running_process()
            .get_memory_manager())
    };

    if address != 0 {
        /* Memory Map */
        pr_warn!("Address({:#X}) will be ignored.", address);
    }
    /* Memory Allocation */
    let result =
        memory_manager.alloc_nonlinear_pages(size, memory_permission, Some(memory_options));
    if let Err(e) = result {
        pr_err!("Failed to allocate memory: {:?}", e);
        return Err(());
    }
    return Ok(result.unwrap().to_usize());
}

fn check_user_address(
    user_address: usize,
    _size: MSize,
    _read: bool,
    _write: bool,
) -> Result<VAddress, ()> {
    Ok(VAddress::new(user_address))
}

#[allow(dead_code)]
fn read_data_from_user(_user_address: VAddress, _size: MSize, _buffer: VAddress) -> Result<(), ()> {
    unimplemented!()
}

#[allow(dead_code)]
fn write_data_into_user(
    _user_address: VAddress,
    _size: MSize,
    _buffer: VAddress,
) -> Result<(), ()> {
    unimplemented!()
}
