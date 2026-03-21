//!
//! System Call Handler
//!

use crate::arch::target_arch::context::context_data::ContextData;
use crate::arch::target_arch::context::memory_layout::is_user_memory_area;
use crate::arch::target_arch::interrupt::InterruptManager;
use crate::arch::target_arch::system_call::{system_call_number::*, *};

use crate::kernel::file_manager::*;
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::{
    data_type::{Address, MOffset, MSize, MemoryOptionFlags, MemoryPermissionFlags, VAddress},
    kfree, kmalloc,
};
use crate::kernel::network_manager::socket_manager::socket_system_call;
use crate::kernel::task_manager::{ProcessStatus, TaskStatus};

use core::ptr::copy_nonoverlapping;

const MAX_PATH_LENGTH: usize = 256;

#[repr(isize)]
#[derive(Debug)]
pub enum ErrorCode {
    Permission = -1,
    NoEntry = -2,
    NoProcess = -3,
    Invalid = -4,
    Io = -5,
    BadFileNumber = -9,
    NoMemory = -12,
    Access = -13,
    Fault = -14,
    Range = -34,
    NameTooLong = -36,
}

pub fn system_call_handler(context: &mut ContextData) {
    match context.get_system_call_arguments(0).unwrap() as SysCallNumber {
        SYSCALL_EXIT => {
            pr_info!(
                "SysCall: Exit(Return Code: {:#X})",
                context.get_system_call_arguments(1).unwrap()
            );
            let irq = InterruptManager::save_and_disable_local_irq();
            let t = get_cpu_manager_cluster().run_queue.get_running_thread();
            t.set_task_status(TaskStatus::Stopped);
            InterruptManager::restore_local_irq(irq);
            get_cpu_manager_cluster().run_queue.schedule(Some(context));
            unreachable!();
        }
        SYSCALL_EXIT_GROUP => {
            pr_info!(
                "SysCall: ExitGroup(Return Code: {:#X})",
                context.get_system_call_arguments(1).unwrap()
            );
            let irq = InterruptManager::save_and_disable_local_irq();
            let t = get_cpu_manager_cluster().run_queue.get_running_thread();
            t.set_task_status(TaskStatus::Stopped);
            let p = unsafe { &mut *t.get_process_mut() };
            p.set_process_status(ProcessStatus::Zombie);
            InterruptManager::restore_local_irq(irq);
            get_cpu_manager_cluster().run_queue.schedule(Some(context));
            unreachable!();
        }
        SYSCALL_WRITE => {
            let process = get_cpu_manager_cluster().run_queue.get_running_process();

            let Some(file) =
                process.get_file(context.get_system_call_arguments(1).unwrap() as usize)
            else {
                context.set_system_call_return_value(ErrorCode::BadFileNumber as _);
                return;
            };
            context.set_system_call_return_value(
                match system_call_write(
                    &mut file.lock().unwrap(),
                    context.get_system_call_arguments(2).unwrap() as usize,
                    context.get_system_call_arguments(3).unwrap() as usize,
                ) {
                    Ok(r) => r as u64,
                    Err(e) => e as u64,
                },
            );
        }
        SYSCALL_WRITEV => {
            let process = get_cpu_manager_cluster().run_queue.get_running_process();

            let Some(file) =
                process.get_file(context.get_system_call_arguments(1).unwrap() as usize)
            else {
                context.set_system_call_return_value(ErrorCode::BadFileNumber as _);
                return;
            };
            let mut file_unlocked = file.lock().unwrap();
            let mut written_bytes = 0usize;
            let iov = context.get_system_call_arguments(2).unwrap() as usize;
            for i in 0..(context.get_system_call_arguments(3).unwrap() as usize) {
                let iovec = iov + i * (size_of::<usize>() * 2);
                if check_user_address(
                    VAddress::new(iovec),
                    MSize::new(size_of::<usize>() * 2),
                    true,
                    false,
                )
                .is_err()
                {
                    pr_err!("{:#X} is not accessible", iovec);
                    context.set_system_call_return_value(ErrorCode::Fault as _);
                    break;
                }
                let iov_base = unsafe { *(iovec as *const usize) };
                let iov_len = unsafe { *((iovec + size_of::<usize>()) as *const usize) };
                if let Ok(bytes) = system_call_write(&mut file_unlocked, iov_base, iov_len) {
                    written_bytes += bytes;
                } else {
                    break;
                }
            }
            drop(file);
            if written_bytes == 0 {
                context.set_system_call_return_value(ErrorCode::Io as _);
            } else {
                context.set_system_call_return_value(written_bytes as u64);
            }
        }
        SYSCALL_READ => {
            let process = get_cpu_manager_cluster().run_queue.get_running_process();
            let Some(file) =
                process.get_file(context.get_system_call_arguments(1).unwrap() as usize)
            else {
                context.set_system_call_return_value(ErrorCode::BadFileNumber as _);
                return;
            };
            let size = MSize::new(context.get_system_call_arguments(3).unwrap() as usize);
            let kernel_buffer = match kmalloc!(size) {
                Ok(a) => a,
                Err(e) => {
                    pr_err!("Failed to allocate memory: {:?}", e);
                    context.set_system_call_return_value(ErrorCode::NoMemory as _);
                    return;
                }
            };
            let result = file.lock().unwrap().read(kernel_buffer, size);
            if result.is_ok()
                && let Err(e) = write_data_into_user(
                    VAddress::new(context.get_system_call_arguments(2).unwrap() as usize),
                    size,
                    kernel_buffer,
                )
            {
                pr_err!("Failed to copy data into user: {e:?}");
                let _ = kfree!(kernel_buffer, size);
                context.set_system_call_return_value(e as _);
                return;
            }
            let _ = kfree!(kernel_buffer, size);
            context.set_system_call_return_value(
                result
                    .map(|r| r.to_usize() as u64)
                    .unwrap_or(ErrorCode::Io as _),
            );
        }
        SYSCALL_OPEN => match system_call_open(context) {
            Ok(fd) => context.set_system_call_return_value(fd as _),
            Err(eno) => context.set_system_call_return_value((eno as isize).cast_unsigned() as _),
        },
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
                    context.set_system_call_return_value(ErrorCode::Invalid as _);
                    return;
                }
            };

            let process = get_cpu_manager_cluster().run_queue.get_running_process();
            if let Some(file) =
                process.get_file(context.get_system_call_arguments(1).unwrap() as usize)
            {
                let result = file.lock().unwrap().seek(
                    MOffset::new(context.get_system_call_arguments(2).unwrap() as usize),
                    seek_origin,
                );
                context.set_system_call_return_value(
                    result
                        .map(|r| r.to_usize() as u64)
                        .unwrap_or(ErrorCode::Io as _),
                );
            } else {
                context.set_system_call_return_value(ErrorCode::BadFileNumber as _);
            }
        }
        SYSCALL_CLOSE => {
            let process = get_cpu_manager_cluster().run_queue.get_running_process();
            if let Some(file) =
                process.get_file(context.get_system_call_arguments(1).unwrap() as usize)
            {
                core::mem::take(&mut *file.lock().unwrap());
                context.set_system_call_return_value(0);
            } else {
                context.set_system_call_return_value(ErrorCode::BadFileNumber as _);
            }
        }
                return;
            }
            core::mem::take(&mut *file.unwrap().lock().unwrap());
            context.set_system_call_return_value(0);
        }
        SYSCALL_ARCH_PRCTL => {
            let v = syscall_arch_prctl(context);
            context.set_system_call_return_value(v);
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
        SYSCALL_GET_PID => {
            let irq = InterruptManager::save_and_disable_local_irq();
            let p = get_cpu_manager_cluster().run_queue.get_running_process();
            context.set_system_call_return_value(p.get_pid() as _);
            InterruptManager::restore_local_irq(irq);
        }
        SYSCALL_GET_UID | SYSCALL_GET_EUID => {
            pr_debug!("UID is not implemented, return 0");
            context.set_system_call_return_value(0);
        }
        SYSCALL_GET_GID | SYSCALL_GET_EGID => {
            pr_debug!("GID is not implemented, return 0");
            context.set_system_call_return_value(0);
        }
        SYSCALL_BRK => {
            let addr = VAddress::new(context.get_system_call_arguments(1).unwrap() as _);
            let irq = InterruptManager::save_and_disable_local_irq();
            let p = get_cpu_manager_cluster().run_queue.get_running_process();
            let result = p.increate_heap(addr);
            InterruptManager::restore_local_irq(irq);

            match result {
                Ok(v) => {
                    context.set_system_call_return_value(v.to_usize() as _);
                }
                Err(e) => {
                    pr_err!("brk({addr}) was failed: {e:?}");
                    context.set_system_call_return_value(ErrorCode::NoMemory as _);
                }
            }
        }
        SYSCALL_MMAP => {
            let address = context.get_system_call_arguments(1).unwrap();
            let size = context.get_system_call_arguments(2).unwrap();
            let prot = context.get_system_call_arguments(3).unwrap_or(0);
            let flags = context.get_system_call_arguments(4).unwrap_or(0);
            let fd = context.get_system_call_arguments(5).unwrap_or(0);
            let offset = context.get_system_call_arguments(6).unwrap_or(0);
            context.set_system_call_return_value(
                match system_call_memory_map(
                    address as usize,
                    size as usize,
                    prot as usize,
                    flags as usize,
                    fd as usize,
                    offset as usize,
                ) {
                    Ok(v) => v as _,
                    Err(e) => e as _,
                },
            );
        }
        SYSCALL_MUNMAP => {
            let address = context.get_system_call_arguments(1).unwrap();
            /* TODO: Check Address */
            let memory_manager = unsafe {
                &mut *(get_cpu_manager_cluster()
                    .run_queue
                    .get_running_process()
                    .get_memory_manager())
            };
            let result = memory_manager.free(VAddress::new(address as usize));
            context.set_system_call_return_value(if let Err(e) = result {
                pr_err!("Failed to free memory: {:?}", e);
                ErrorCode::NoMemory as _
            } else {
                0
            });
        }
        SYSCALL_SOCKET => {
            let domain_number = context.get_system_call_arguments(1).unwrap();
            let socket_type_number = context.get_system_call_arguments(2).unwrap();
            let protocol_number = context.get_system_call_arguments(3).unwrap();
            let Ok(socket) = socket_system_call::create_socket(
                domain_number,
                socket_type_number,
                protocol_number,
            ) else {
                pr_warn!("Failed to create socket");
                context.set_system_call_return_value(ErrorCode::Io as _);
                return;
            };
            let process = get_cpu_manager_cluster().run_queue.get_running_process();
            let fd = process.add_file(socket);
            context.set_system_call_return_value(fd as u64);
        }
        SYSCALL_BIND => {
            let process = get_cpu_manager_cluster().run_queue.get_running_process();
            let Some(file) =
                process.get_file(context.get_system_call_arguments(1).unwrap() as usize)
            else {
                context.set_system_call_return_value(ErrorCode::BadFileNumber as _);
                return;
            };
            let sock_addr_address = context.get_system_call_arguments(2).unwrap();
            let sock_addr_size = context.get_system_call_arguments(3).unwrap();
            if sock_addr_size as usize != size_of::<socket_system_call::SockAddr>() {
                pr_debug!("Unsupported the size of SockAddr: {sock_addr_size}");
                context.set_system_call_return_value(ErrorCode::Invalid as _);
                return;
            }
            if socket_system_call::bind_socket(&mut file.lock().unwrap(), unsafe {
                &*(sock_addr_address as usize as *const socket_system_call::SockAddr)
            })
            .is_ok()
            {
                context.set_system_call_return_value(0);
            } else {
                context.set_system_call_return_value(ErrorCode::Io as _);
            }
        }
        SYSCALL_LISTEN => {
            let process = get_cpu_manager_cluster().run_queue.get_running_process();
            let Some(file) =
                process.get_file(context.get_system_call_arguments(1).unwrap() as usize)
            else {
                context.set_system_call_return_value(ErrorCode::BadFileNumber as _);
                return;
            };
            let max_connection = context.get_system_call_arguments(2).unwrap();
            if let Err(err) = socket_system_call::listen_socket(
                &mut file.lock().unwrap(),
                max_connection as usize,
            ) {
                pr_err!("Failed to listen socket: {:?}", err);
                context.set_system_call_return_value(ErrorCode::Io as _);
                return;
            }
            context.set_system_call_return_value(0);
        }
        SYSCALL_ACCEPT => {
            let process = get_cpu_manager_cluster().run_queue.get_running_process();
            let Some(file) =
                process.get_file(context.get_system_call_arguments(1).unwrap() as usize)
            else {
                context.set_system_call_return_value(ErrorCode::BadFileNumber as _);
                return;
            };
            //let sock_addr_address = context.get_system_call_arguments(2).unwrap();
            //let sock_addr_size_address = context.get_system_call_arguments(3).unwrap();
            /*if sock_addr_size as usize != size_of::<socket_system_call::SockAddr>() {
                pr_debug!("Unsupported the size of SockAddr: {sock_addr_size}");
                context.set_system_call_return_value(SYSCALL_RETURN_ERROR);
                return;
            }*/
            let result = socket_system_call::accept(&mut file.lock().unwrap());
            if let Err(err) = result {
                pr_debug!("Failed to accept connection: {:?}", err);
                context.set_system_call_return_value(ErrorCode::Io as _);
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
            let Some(file) =
                process.get_file(context.get_system_call_arguments(1).unwrap() as usize)
            else {
                context.set_system_call_return_value(ErrorCode::BadFileNumber as _);
                return;
            };
            let buffer_size = MSize::new(context.get_system_call_arguments(3).unwrap() as usize);
            let buffer_address = match check_user_address(
                VAddress::new(context.get_system_call_arguments(2).unwrap() as usize),
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
                    context.set_system_call_return_value(ErrorCode::Invalid as _);
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
                    context.set_system_call_return_value(ErrorCode::Io as _);
                }
            }
        }
        SYSCALL_SENDTO => {
            let process = get_cpu_manager_cluster().run_queue.get_running_process();
            let Some(file) =
                process.get_file(context.get_system_call_arguments(1).unwrap() as usize)
            else {
                context.set_system_call_return_value(ErrorCode::BadFileNumber as _);
                return;
            };
            let buffer_size = MSize::new(context.get_system_call_arguments(3).unwrap() as usize);
            if buffer_size.is_zero() {
                context.set_system_call_return_value(ErrorCode::Invalid as _);
            }
            let buffer_address = match check_user_address(
                VAddress::new(context.get_system_call_arguments(2).unwrap() as usize),
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
                    context.set_system_call_return_value(ErrorCode::Invalid as _);
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
                    context.set_system_call_return_value(ErrorCode::Io as _);
                }
            }
        }
        s => {
            if !arch_system_call_handler(s, context) {
                pr_err!("Unknown System Call: {s}");
                context.set_system_call_return_value(ErrorCode::Fault as _);
            }
        }
    }
}

fn system_call_open(context: &mut ContextData) -> Result<usize, ErrorCode> {
    const O_ACCMODE: u64 = 3;
    const O_RDONLY: u64 = 0;
    const O_WRONLY: u64 = 1;
    const O_RDWR: u64 = 2;
    const O_DIRECTORY: u64 = 0o200000;
    // const O_LARGEFILE: u64 = 0o0100000;

    let mut str_buffer = [0; MAX_PATH_LENGTH];
    let file_name = read_str_from_user(
        VAddress::new(
            context
                .get_system_call_arguments(1)
                .ok_or(ErrorCode::Invalid)? as usize,
        ),
        &mut str_buffer,
    )
    .or(Err(ErrorCode::Invalid))?;
    let flag = context
        .get_system_call_arguments(2)
        .ok_or(ErrorCode::Invalid)?;

    let mut permission;
    let mut open_flags = 0;
    match flag & O_ACCMODE {
        O_RDONLY => {
            permission = FILE_PERMISSION_READ;
        }
        O_WRONLY => {
            permission = FILE_PERMISSION_WRITE;
        }
        O_RDWR => {
            permission = FILE_PERMISSION_READ | FILE_PERMISSION_WRITE;
        }
        _ => {
            return Err(ErrorCode::Invalid);
        }
    }
    if (flag & O_DIRECTORY) != 0 {
        permission |= FILE_PERMISSION_DIRECTORY;
        open_flags |= FILE_FLAGS_RESTRICT_MODE;
    }
    /* TODO: Current Directory*/
    if let Ok(f) = get_kernel_manager_cluster().file_manager.open_file(
        PathInfo::new(file_name),
        None,
        permission,
        open_flags,
    ) {
        let fd = get_cpu_manager_cluster()
            .run_queue
            .get_running_process()
            .add_file(f);
        Ok(fd)
    } else {
        pr_warn!("{} is not found.", file_name);
        Err(ErrorCode::NoEntry)
    }
}

fn system_call_write(file: &mut File, user_address: usize, len: usize) -> Result<usize, ErrorCode> {
    if user_address == 0 {
        return if len == 0 {
            Ok(0)
        } else {
            Err(ErrorCode::Fault)
        };
    } else if len == 0 {
        return Ok(0);
    }
    let size = MSize::new(len);
    let kernel_buffer = kmalloc!(size).or_else(|e| {
        pr_err!("Failed to allocate memory: {:?}", e);
        Err(ErrorCode::NoMemory)
    })?;
    read_data_from_user(VAddress::new(user_address), size, kernel_buffer)?;

    let result = file.write(kernel_buffer, size);
    let _ = kfree!(kernel_buffer, size);
    result.map(|s| s.to_usize()).map_err(|err| {
        pr_err!("Failed to write: {:?}", err);
        ErrorCode::Io
    })
}

fn system_call_memory_map(
    address: usize,
    size: usize,
    prot: usize,
    flags: usize,
    _fd: usize,
    _offset: usize,
) -> Result<usize, ErrorCode> {
    /* PROT */
    const PROT_NONE: usize = 0x00;
    const PROT_READ: usize = 0x01;
    const PROT_WRITE: usize = 0x02;
    const PROT_EXEC: usize = 0x04;

    /* FLAGS */
    const MAP_SHARED: usize = 0x01;
    const MAP_PRIVATE: usize = 0x02;
    //const MAP_FIXED: usize = 0x10;
    const MAP_ANONYMOUS: usize = 0x20;

    if size == 0 {
        return Err(ErrorCode::Invalid);
    }
    let address = VAddress::new(address);
    let size = MSize::new(size).page_align_up();

    let memory_permission = MemoryPermissionFlags::new(
        (prot & PROT_READ) != 0,
        (prot & PROT_WRITE) != 0,
        (prot & PROT_EXEC) != 0,
        prot != PROT_NONE,
    );
    if (flags & MAP_ANONYMOUS) == 0 {
        pr_err!("Flags({:#X}) is not anonymous.", flags);
        return Err(ErrorCode::Invalid);
    }
    if (flags & MAP_SHARED) != 0 {
        pr_warn!("Shared mapping is requested, but not implemented.");
    }
    if (flags & MAP_PRIVATE) != 0 {
        pr_warn!("CoW mapping is requested, but not implemented.");
    }

    let memory_options = MemoryOptionFlags::ALLOC | MemoryOptionFlags::USER;

    let memory_manager = unsafe {
        &mut *(get_cpu_manager_cluster()
            .run_queue
            .get_running_process()
            .get_memory_manager())
    };

    let result: Result<VAddress, ErrorCode>;
    if address.is_zero() {
        /* Memory Allocation */
        result = memory_manager
            .alloc_nonlinear_pages(size, memory_permission, Some(memory_options))
            .map_err(|e| {
                pr_err!("Failed to allocate memory: {e:?}");
                ErrorCode::NoMemory
            });
        if let Ok(address) = result
            && (flags & MAP_ANONYMOUS) != 0
        {
            /* the memory area allocated with `MAP_ANONYMOUS` must be zero cleared */
            unsafe { core::ptr::write_bytes(address.to::<u8>(), 0, size.to_usize()) };
        }
    } else {
        /* brk fast path */
        /* TODO: make brk lazy mapping */
        let irq = InterruptManager::save_and_disable_local_irq();
        let p = get_cpu_manager_cluster().run_queue.get_running_process();
        let (heap, heap_size) = p.get_heap_area();
        InterruptManager::restore_local_irq(irq);
        if heap <= address
            && (address + size) <= (heap + heap_size)
            && !memory_permission.is_user_accessible()
        {
            result = Ok(address);
        } else {
            pr_warn!("Mapping memory is not supported");
            result = Err(ErrorCode::Fault);
        }
    }
    result.map(|v| v.to_usize())
}

fn check_user_address(
    user_address: VAddress,
    size: MSize,
    _read: bool,
    _write: bool,
) -> Result<VAddress, ErrorCode> {
    if user_address.is_zero() {
        return Err(ErrorCode::Fault);
    }
    if !is_user_memory_area(user_address) || !is_user_memory_area(user_address + size) {
        return Err(ErrorCode::Fault);
    }
    /*TODO: valid address check including read/write */
    Ok(user_address)
}

fn read_data_from_user(
    user_address: VAddress,
    size: MSize,
    buffer: VAddress,
) -> Result<(), ErrorCode> {
    let a = check_user_address(user_address, size, true, false)?;
    /* Assume the user address exists on the memory(not swapped out) */
    unsafe { copy_nonoverlapping(a.to::<u8>(), buffer.to::<u8>(), size.to_usize()) };
    Ok(())
}

fn read_str_from_user(user_address: VAddress, buffer: &mut [u8]) -> Result<&str, ErrorCode> {
    read_data_from_user(
        user_address,
        MSize::new(buffer.len()),
        VAddress::new(buffer.as_ptr() as usize),
    )?;
    let end = buffer
        .iter()
        .position(|&b| b == 0)
        .ok_or(())
        .or(Err(ErrorCode::NameTooLong))?;
    core::str::from_utf8(&buffer[0..end]).map_err(|_| ErrorCode::NoEntry)
}

fn write_data_into_user(
    user_address: VAddress,
    size: MSize,
    buffer: VAddress,
) -> Result<(), ErrorCode> {
    let user_address = check_user_address(user_address, size, false, true)?;
    /* Assume the user address exists on the memory(not swapped out) */
    unsafe { copy_nonoverlapping(buffer.to::<u8>(), user_address.to::<u8>(), size.to_usize()) };
    Ok(())
}

fn write_str_into_user(user_address: VAddress, user_size: MSize, s: &str) -> Result<(), ErrorCode> {
    let write_size = MSize::new(s.len() + 1);
    if user_size > write_size {
        return Err(ErrorCode::Range);
    }
    let a = check_user_address(user_address, MSize::new(s.len() + 1), false, true)?;
    /* Assume the user address exists on the memory(not swapped out) */
    unsafe { copy_nonoverlapping(s.as_ptr(), a.to::<u8>(), s.len()) };
    unsafe { core::ptr::write_volatile((a + MSize::new(s.len())).to::<u8>(), 0) };
    Ok(())
}
