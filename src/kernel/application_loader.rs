//!
//! Application Software Loader
//!

use crate::arch::target_arch::context::memory_layout::USER_STACK_END_ADDRESS;
use crate::arch::target_arch::context::ContextManager;
use crate::arch::target_arch::paging::PAGE_SIZE_USIZE;

use crate::kernel::collections::auxiliary_vector;
use crate::kernel::file_manager::elf::{Elf64Header, ELF_PROGRAM_HEADER_SEGMENT_LOAD};
use crate::kernel::file_manager::{
    FileSeekOrigin, PathInfo, FILE_PERMISSION_READ, FILE_PERMISSION_WRITE,
};
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{
    Address, MOffset, MSize, MemoryOptionFlags, MemoryPermissionFlags, VAddress,
};
use crate::kernel::memory_manager::{
    alloc_non_linear_pages, free_pages, kfree, kmalloc, MemoryManager,
};

const DEFAULT_PRIVILEGE_LEVEL: u8 = 3;
const DEFAULT_PRIORITY_LEVEL: u8 = 2;

pub fn load_and_execute(
    file_name: &str,
    arguments: &[&str],
    environments: &[(&str, &str)],
    elf_machine_type: u16,
) -> Result<(), ()> {
    pr_debug!("Search {}", file_name);
    let result = get_kernel_manager_cluster().file_manager.open_file(
        PathInfo::new(file_name),
        None,
        FILE_PERMISSION_READ,
    );
    if let Err(e) = result {
        pr_err!("{} is not found: {:?}", file_name, e);
        return Err(());
    }
    let mut file_descriptor = result.unwrap();

    let head_read_size = MSize::new(1024);
    let head_data = match kmalloc!(head_read_size) {
        Ok(v) => v,
        Err(e) => {
            pr_err!("Failed to allocate memory: {:?}", e);
            let _ = file_descriptor.close();
            return Err(());
        }
    };
    if let Err(e) = file_descriptor.read(head_data, head_read_size) {
        pr_err!("Failed to read data: {:?}", e);
        let _ = file_descriptor.close();
        let _ = kfree!(head_data, head_read_size);
        return Err(());
    }

    let header = match unsafe { Elf64Header::from_address(head_data.to_usize() as *const u8) } {
        Ok(e) => e,
        Err(e) => {
            pr_err!("File is not valid ELF file: {:?}", e);
            let _ = file_descriptor.close();
            let _ = kfree!(head_data, head_read_size);
            return Err(());
        }
    };
    if !header.is_executable_file()
        || header.get_machine_type() != elf_machine_type
        || !header.is_lsb()
    {
        pr_err!("The file is not executable.");
        let _ = file_descriptor.close();
        let _ = kfree!(head_data, head_read_size);
        return Err(());
    }

    if (header.get_program_header_offset() + header.get_program_header_array_size()) as usize
        > head_read_size.to_usize()
    {
        pr_err!("Program Header is too far from head(TODO: support...)");
        let _ = file_descriptor.close();
        let _ = kfree!(head_data, head_read_size);
        return Err(());
    }

    let process = match get_kernel_manager_cluster()
        .task_manager
        .create_user_process(core::ptr::null_mut(), DEFAULT_PRIVILEGE_LEVEL)
    {
        Ok(e) => e,
        Err(e) => {
            pr_err!("Failed to create the user process: {:?}", e);
            let _ = file_descriptor.close();
            let _ = kfree!(head_data, head_read_size);
            return Err(());
        }
    };
    let process_memory_manager = unsafe { &mut *process.get_memory_manager() };

    let result: Result<(), ()> = try {
        for program_header in header.get_program_header_iter(
            head_data.to_usize() + header.get_program_header_offset() as usize,
        ) {
            /* TODO: delete the process when failed. */
            if program_header.get_segment_type() == ELF_PROGRAM_HEADER_SEGMENT_LOAD {
                pr_debug!(
                "PA: {:#X}, VA: {:#X}, MS: {:#X}, FS: {:#X}, FO: {:#X}, AL: {}, R:{}, W: {}, E:{}",
                program_header.get_physical_address(),
                program_header.get_virtual_address(),
                program_header.get_memory_size(),
                program_header.get_file_size(),
                program_header.get_file_offset(),
                program_header.get_align(),
                program_header.is_segment_readable(),
                program_header.is_segment_writable(),
                program_header.is_segment_executable()
            );

                let alignment = program_header.get_align().max(1);
                let align_offset =
                    MSize::new((program_header.get_virtual_address() & (alignment - 1)) as usize);
                if alignment != 1
                    && (align_offset.to_usize()
                        != (program_header.get_file_offset() & (alignment - 1)) as usize
                        || !alignment.is_power_of_two())
                {
                    pr_err!("Invalid Alignment: {:#X}", alignment);
                    Err(())?
                } else if alignment as usize > PAGE_SIZE_USIZE {
                    pr_err!("Unsupported Align: {:#X}", alignment);
                    Err(())?
                } else if program_header.get_memory_size() == 0 {
                    continue;
                }

                let aligned_memory_size = MemoryManager::size_align(
                    MSize::new(program_header.get_memory_size() as usize) + align_offset,
                );
                let allocated_memory = match alloc_non_linear_pages!(
                    aligned_memory_size,
                    MemoryPermissionFlags::data()
                ) {
                    Ok(v) => v,
                    Err(e) => {
                        pr_err!("Failed to allocate memory: {:?}", e);
                        Err(())?
                    }
                };
                if program_header.get_file_size() > 0 {
                    if let Err(e) = file_descriptor.seek(
                        MOffset::new(program_header.get_file_offset() as usize),
                        FileSeekOrigin::SeekSet,
                    ) {
                        pr_err!("Failed to seek: {:?}", e);
                        let _ = free_pages!(allocated_memory);
                        Err(())?
                    }
                    if let Err(e) = file_descriptor.read(
                        allocated_memory + align_offset,
                        MSize::new(program_header.get_file_size() as usize),
                    ) {
                        pr_err!("Failed to read data: {:?}", e);
                        let _ = free_pages!(allocated_memory);
                        Err(())?
                    }
                }
                if program_header.get_memory_size() > program_header.get_file_size() {
                    unsafe {
                        core::ptr::write_bytes(
                            ((allocated_memory + align_offset).to_usize()
                                + program_header.get_file_size() as usize)
                                as *mut u8,
                            0,
                            (program_header.get_memory_size() - program_header.get_file_size())
                                as usize,
                        )
                    }
                }
                if let Err(e) = get_kernel_manager_cluster()
                    .kernel_memory_manager
                    .share_kernel_memory_with_user(
                        process_memory_manager,
                        allocated_memory,
                        VAddress::new(program_header.get_virtual_address() as usize) - align_offset,
                        MemoryPermissionFlags::new(
                            program_header.is_segment_readable(),
                            program_header.is_segment_writable(),
                            program_header.is_segment_executable(),
                            true,
                        ),
                        MemoryOptionFlags::USER,
                    )
                {
                    pr_err!("Failed to map memory into user process: {:?}", e);
                    let _ = free_pages!(allocated_memory);
                    Err(())?
                }

                let _ = free_pages!(allocated_memory);
            }
        }
    };
    let _ = file_descriptor.close();

    if result.is_err() {
        let _ = kfree!(head_data, head_read_size);
        if let Err(e) = get_kernel_manager_cluster()
            .task_manager
            .delete_user_process(process)
        {
            pr_err!("Failed to delete user process: {:?}", e);
        }
        return Err(());
    }
    let stack_size = MSize::new(ContextManager::DEFAULT_STACK_SIZE_OF_USER);
    let stack_address = match alloc_non_linear_pages!(stack_size) {
        Ok(v) => v,
        Err(e) => {
            pr_err!("Failed to alloc stack: {:?}", e);
            let _ = kfree!(head_data, head_read_size);
            if let Err(e) = get_kernel_manager_cluster()
                .task_manager
                .delete_user_process(process)
            {
                pr_err!("Failed to delete user process: {:?}", e);
            }
            return Err(());
        }
    };

    /* Build Arguments */
    let stack_top_address = (stack_address + stack_size).to_usize();

    /* Auxiliary Vector */
    let auxiliary_vector_list: [auxiliary_vector::AuxiliaryVector; 1] =
        [auxiliary_vector::AuxiliaryVector {
            aux_type: auxiliary_vector::AT_NULL,
            value: 0,
        }];

    /* Calculate the position of "ap" for _start */
    let mut ap_offset_from_stack_top = 0;
    ap_offset_from_stack_top += file_name.as_bytes().len() + 1;
    for e in arguments {
        ap_offset_from_stack_top += e.as_bytes().len() + 1;
    }
    for e in environments {
        ap_offset_from_stack_top += e.0.as_bytes().len() + 1 + e.1.as_bytes().len() + 1;
    }
    ap_offset_from_stack_top +=
        auxiliary_vector_list.len() * core::mem::size_of::<auxiliary_vector::AuxiliaryVector>();
    if (ap_offset_from_stack_top & 0b111) != 0 {
        ap_offset_from_stack_top = (ap_offset_from_stack_top & !0b111) + 8;
    }
    ap_offset_from_stack_top += (1 /* argc */+ 1 /* file_name */ + arguments.len() + 1 + environments.len() + 1)
        * core::mem::size_of::<u64>();

    let ap_offset_from_stack_top = ap_offset_from_stack_top;
    let stack_top_address_user = USER_STACK_END_ADDRESS.to_usize() + 1;
    let mut ap = stack_top_address - ap_offset_from_stack_top;
    let mut argv_env_pointer = 0;

    /* Write argc */
    unsafe {
        *(ap as *mut u64) = 1 /* file_name */ +  arguments.len() as u64
    };
    ap += core::mem::size_of::<u64>();

    /* Write arguments */
    for e in [file_name].iter().chain(arguments.iter()) {
        let len = e.as_bytes().len();
        unsafe {
            core::ptr::copy_nonoverlapping(
                e.as_bytes().as_ptr(),
                (stack_top_address - argv_env_pointer - len - 1) as *mut u8,
                len,
            );
            *((stack_top_address - argv_env_pointer - 1) as *mut u8) = 0;
        }
        argv_env_pointer += len + 1;
        unsafe { *(ap as *mut u64) = (stack_top_address_user - argv_env_pointer) as u64 };
        ap += core::mem::size_of::<u64>();
    }
    unsafe { *(ap as *mut u64) = 0 as u64 };
    ap += core::mem::size_of::<u64>();

    /* Write environment variables */
    for e in environments {
        let mut len = e.0.as_bytes().len() + 1 + e.1.as_bytes().len();
        unsafe {
            core::ptr::copy_nonoverlapping(
                e.0.as_bytes().as_ptr(),
                (stack_top_address - argv_env_pointer - len - 1) as *mut u8,
                e.0.as_bytes().len(),
            );
            len -= e.0.as_bytes().len();
            *((stack_top_address - argv_env_pointer - len - 1) as *mut u8) = b'=';
            len -= 1;
            core::ptr::copy_nonoverlapping(
                e.1.as_bytes().as_ptr(),
                (stack_top_address - argv_env_pointer - len - 1) as *mut u8,
                e.1.as_bytes().len(),
            );
            *((stack_top_address - argv_env_pointer - 1) as *mut u8) = 0;
        }
        argv_env_pointer += e.0.as_bytes().len() + 1 + e.1.as_bytes().len() + 1;
        unsafe { *(ap as *mut u64) = (stack_top_address_user - argv_env_pointer) as u64 };
        ap += core::mem::size_of::<u64>();
    }
    unsafe { *(ap as *mut u64) = 0 as u64 };

    assert!(ap < (stack_top_address - argv_env_pointer));

    /* Write auxiliary vector */
    for e in auxiliary_vector_list {
        unsafe { *(ap as *mut auxiliary_vector::AuxiliaryVector) = e };
        ap += core::mem::size_of::<auxiliary_vector::AuxiliaryVector>();
    }

    if let Err(e) = get_kernel_manager_cluster()
        .kernel_memory_manager
        .share_kernel_memory_with_user(
            process_memory_manager,
            stack_address,
            VAddress::new(stack_top_address_user) - stack_size,
            MemoryPermissionFlags::new(true, true, false, true),
            MemoryOptionFlags::USER | MemoryOptionFlags::STACK,
        )
    {
        pr_err!("Failed to map stack into user: {:?}", e);
        let _ = free_pages!(stack_address);
        let _ = kfree!(head_data, head_read_size);
        if let Err(e) = get_kernel_manager_cluster()
            .task_manager
            .delete_user_process(process)
        {
            pr_err!("Failed to delete user process: {:?}", e);
        }
        return Err(());
    }
    let _ = free_pages!(stack_address);

    let thread = get_kernel_manager_cluster()
        .task_manager
        .create_user_thread(
            process,
            header.get_entry_point() as usize,
            &[stack_top_address_user - ap_offset_from_stack_top],
            VAddress::new(stack_top_address_user - ap_offset_from_stack_top),
            DEFAULT_PRIORITY_LEVEL,
        );
    if let Err(e) = thread {
        pr_err!("Failed to add thread: {:?}", e);
        let _ = kfree!(head_data, head_read_size);
        if let Err(e) = get_kernel_manager_cluster()
            .task_manager
            .delete_user_process(process)
        {
            pr_err!("Failed to delete user process: {:?}", e);
        }
        return Err(());
    }
    let _ = kfree!(head_data, head_read_size);

    /* Add stdout/stdin */
    process.add_file(
        get_kernel_manager_cluster()
            .kernel_tty_manager
            .open_tty_as_file(FILE_PERMISSION_READ)
            .unwrap(),
    ); /* stdin */
    process.add_file(
        get_kernel_manager_cluster()
            .kernel_tty_manager
            .open_tty_as_file(FILE_PERMISSION_WRITE)
            .unwrap(),
    ); /* stderr */
    process.add_file(
        get_kernel_manager_cluster()
            .kernel_tty_manager
            .open_tty_as_file(FILE_PERMISSION_WRITE)
            .unwrap(),
    ); /* stderr */

    pr_debug!("Execute {}", file_name);
    if let Err(e) = get_kernel_manager_cluster()
        .task_manager
        .wake_up_thread(thread.unwrap())
    {
        pr_err!("Failed to run the thread: {:?}", e);
        return Err(());
    }
    return Ok(());
}
