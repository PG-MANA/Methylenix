//!
//! Init codes for device, memory and else based on MultibootInformation
//!
//! This module is called by boot function.

use super::MEMORY_FOR_PHYSICAL_MEMORY_MANAGER;

use crate::arch::target_arch::paging::{PAGE_MASK, PAGE_SHIFT, PAGE_SIZE};

use crate::kernel::drivers::multiboot::MultiBootInformation;
use crate::kernel::graphic_manager::font::FontType;
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{Address, MSize, PAddress, VAddress};
use crate::kernel::memory_manager::kernel_malloc_manager::KernelMemoryAllocManager;
use crate::kernel::memory_manager::physical_memory_manager::PhysicalMemoryManager;
use crate::kernel::memory_manager::virtual_memory_manager::VirtualMemoryManager;
use crate::kernel::memory_manager::{
    MemoryOptionFlags, MemoryPermissionFlags, SystemMemoryManager,
};
use crate::kernel::sync::spin_lock::Mutex;

use core::mem;

/// Init memory system based on multiboot information.
/// This function set up PhysicalMemoryManager which manages where is free
/// and VirtualMemoryManager which manages which process is using what area of virtual memory.
/// After that, this will set up MemoryManager.
/// If one of process is failed, this will panic.
/// This function returns new address of MultiBootInformation.
pub fn init_memory_by_multiboot_information(
    multiboot_information: MultiBootInformation,
) -> MultiBootInformation {
    /* set up Physical Memory Manager */
    let mut physical_memory_manager = PhysicalMemoryManager::new();
    unsafe {
        physical_memory_manager.set_memory_entry_pool(
            &MEMORY_FOR_PHYSICAL_MEMORY_MANAGER as *const _ as usize,
            mem::size_of_val(&MEMORY_FOR_PHYSICAL_MEMORY_MANAGER),
        );
    }
    for entry in multiboot_information.memory_map_info.clone() {
        if entry.m_type == 1 {
            /* available memory */
            physical_memory_manager.free(
                (entry.addr as usize).into(),
                (entry.length as usize).into(),
                true,
            );
        }
        let area_name = match entry.m_type {
            1 => "available",
            3 => "ACPI information",
            4 => "reserved(must save on hibernation)",
            5 => "defective RAM",
            _ => "reserved",
        };
        pr_info!(
            "[{:#016X}~{:#016X}] {}",
            entry.addr as usize,
            MSize::from(entry.length as usize)
                .to_end_address(PAddress::from(entry.addr as usize))
                .to_usize(),
            area_name
        );
    }
    /* reserve kernel code and data area to avoid using this area */
    for section in multiboot_information.elf_info.clone() {
        if section.should_allocate() && section.align_size() == PAGE_SIZE {
            physical_memory_manager.reserve_memory(
                section.addr().into(),
                section.size().into(),
                PAGE_SHIFT.into(),
            );
        }
    }
    /* reserve Multiboot Information area */
    physical_memory_manager.reserve_memory(
        multiboot_information.address.into(),
        multiboot_information.size.into(),
        0.into(),
    );
    /* reserve Multiboot modules area */
    for e in multiboot_information.modules.iter() {
        if e.start_address != 0 && e.end_address != 0 {
            physical_memory_manager.reserve_memory(
                e.start_address.into(),
                (e.end_address - e.start_address).into(),
                0.into(),
            );
        }
    }

    /* set up Virtual Memory Manager */
    let mut virtual_memory_manager = VirtualMemoryManager::new();
    virtual_memory_manager.init(true, &mut physical_memory_manager);
    for section in multiboot_information.elf_info.clone() {
        if !section.should_allocate() || section.align_size() != PAGE_SIZE {
            continue;
        }
        assert_eq!(!PAGE_MASK & section.addr(), 0);
        let aligned_size = ((section.size() + section.addr() - 1) & PAGE_MASK) + PAGE_SIZE;
        let permission = MemoryPermissionFlags::new(
            true,
            section.should_writable(),
            section.should_excusable(),
            false,
        );
        /* 初期化の段階で1 << order 分のメモリ管理を行ってはいけない。他の領域と重なる可能性がある。*/
        match virtual_memory_manager.map_address(
            PAddress::from(section.addr()),
            Some(VAddress::from(section.addr())),
            aligned_size.into(),
            permission,
            MemoryOptionFlags::new(MemoryOptionFlags::NORMAL),
            &mut physical_memory_manager,
        ) {
            Ok(address) => {
                if address == section.addr().into() {
                    continue;
                }
                pr_err!(
                    "Virtual Address is different from Physical Address.\nV:{:#X} P:{:#X}",
                    address.to_usize(),
                    section.addr()
                );
            }
            Err(e) => {
                pr_err!("Mapping ELF Section was failed. Err:{:?}", e);
            }
        };
        panic!("Cannot map virtual memory correctly.");
    }
    /* set up Memory Manager */
    get_kernel_manager_cluster().system_memory_manager =
        SystemMemoryManager::new(physical_memory_manager);
    let mut memory_manager = get_kernel_manager_cluster()
        .system_memory_manager
        .create_new_memory_manager(virtual_memory_manager);

    /* set up Kernel Memory Alloc Manager */
    let mut kernel_memory_alloc_manager = KernelMemoryAllocManager::new();
    kernel_memory_alloc_manager.init(&mut memory_manager);

    /* move Multiboot Information to allocated memory area */
    let mutex_memory_manager = Mutex::new(memory_manager);
    let new_mbi_address = kernel_memory_alloc_manager
        .kmalloc(
            multiboot_information.size.into(),
            3.into(),
            &mutex_memory_manager,
        )
        .expect("Cannot alloc memory for Multiboot Information.");
    unsafe {
        core::ptr::copy_nonoverlapping(
            multiboot_information.address as *const u8,
            new_mbi_address.to_usize() as *mut u8,
            multiboot_information.size,
        )
    };

    /* free old MultiBootInformation area */
    mutex_memory_manager.lock().unwrap().free_physical_memory(
        multiboot_information.address.into(),
        multiboot_information.size.into(),
    ); /* may be already freed */
    /* apply paging */
    mutex_memory_manager.lock().unwrap().set_paging_table();

    /* store managers to cluster */
    get_kernel_manager_cluster().memory_manager = mutex_memory_manager;
    get_kernel_manager_cluster().kernel_memory_alloc_manager =
        Mutex::new(kernel_memory_alloc_manager);
    MultiBootInformation::new(new_mbi_address.to_usize(), false)
}

/// Init GraphicManager with ModuleInfo of MultibootInformation
///
/// This function loads font data from module info of MultibootInformation.
/// And clear screen.
pub fn init_graphic(multiboot_information: &MultiBootInformation) {
    if get_kernel_manager_cluster().graphic_manager.is_text_mode() {
        return;
    }

    /* load font */
    for module in multiboot_information.modules.iter() {
        if module.name == "font.pf2" {
            let vm_address = get_kernel_manager_cluster()
                .memory_manager
                .lock()
                .unwrap()
                .mmap(
                    module.start_address.into(),
                    (module.end_address - module.start_address).into(),
                    MemoryPermissionFlags::rodata(),
                    MemoryOptionFlags::new(MemoryOptionFlags::NORMAL),
                    false,
                );
            if let Ok(vm_address) = vm_address {
                let result = get_kernel_manager_cluster().graphic_manager.load_font(
                    vm_address,
                    module.end_address - module.start_address,
                    FontType::Pff2,
                );
                if !result {
                    pr_err!("Cannot load font data!");
                }
            } else {
                pr_err!("mapping font data was failed: {:?}", vm_address.err());
            }
        }
    }
}
