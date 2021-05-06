//!
//! Init codes for device, memory and else based on MultibootInformation
//!
//! This module is called by boot function.

use super::MEMORY_FOR_PHYSICAL_MEMORY_MANAGER;

use crate::arch::target_arch::paging::{PAGE_MASK, PAGE_SHIFT, PAGE_SIZE, PAGE_SIZE_USIZE};

use crate::kernel::drivers::efi::boot_service::memory_map::EfiMemoryType;
use crate::kernel::drivers::multiboot::MultiBootInformation;
use crate::kernel::graphic_manager::font::FontType;
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::data_type::{Address, MOrder, MSize, PAddress, VAddress};
use crate::kernel::memory_manager::object_allocator::ObjectAllocator;
use crate::kernel::memory_manager::physical_memory_manager::PhysicalMemoryManager;
use crate::kernel::memory_manager::virtual_memory_manager::VirtualMemoryManager;
use crate::kernel::memory_manager::{
    data_type::MemoryOptionFlags, data_type::MemoryPermissionFlags, MemoryManager,
    SystemMemoryManager,
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
    /* Set up Physical Memory Manager */
    let mut physical_memory_manager = PhysicalMemoryManager::new();
    unsafe {
        physical_memory_manager.set_memory_entry_pool(
            &MEMORY_FOR_PHYSICAL_MEMORY_MANAGER as *const _ as usize,
            mem::size_of_val(&MEMORY_FOR_PHYSICAL_MEMORY_MANAGER),
        );
    }
    for entry in multiboot_information.memory_map_info.clone() {
        if entry.m_type == 1 {
            /* Available memory */
            physical_memory_manager.free(
                PAddress::new(entry.addr as usize),
                MSize::new(entry.length as usize),
                true,
            );
        }
        let area_name = match entry.m_type {
            1 => "Available",
            3 => "ACPI information",
            4 => "Reserved(must save on hibernation)",
            5 => "Defective RAM",
            _ => "Reserved",
        };
        pr_info!(
            "[{:#016X}~{:#016X}] {}",
            entry.addr,
            MSize::new(entry.length as usize)
                .to_end_address(PAddress::new(entry.addr as usize))
                .to_usize(),
            area_name
        );
    }

    /* Reserve EFI Memory Area */
    for entry in multiboot_information.efi_memory_map_info.clone() {
        match entry.memory_type {
            EfiMemoryType::EfiReservedMemoryType|
            EfiMemoryType::EfiBootServicesData/* for BGRT */ |
            EfiMemoryType::EfiRuntimeServicesCode |
            EfiMemoryType::EfiRuntimeServicesData |
            EfiMemoryType::EfiUnusableMemory |
            EfiMemoryType::EfiACPIReclaimMemory|
            EfiMemoryType::EfiACPIMemoryNVS |
            EfiMemoryType::EfiMemoryMappedIO |
            EfiMemoryType::EfiMemoryMappedIOPortSpace |
            EfiMemoryType::EfiPalCode |
            EfiMemoryType::EfiPersistentMemory => {
                physical_memory_manager.reserve_memory(PAddress::new(entry.physical_start),MSize::new((entry.number_of_pages as usize) << PAGE_SHIFT),MOrder::new(0));

            }
            _=>{}
        }
        pr_info!(
            "[{:#016X}~{:#016X}] {}",
            entry.physical_start,
            MSize::new((entry.number_of_pages as usize) << PAGE_SHIFT)
                .to_end_address(PAddress::new(entry.physical_start as usize))
                .to_usize(),
            entry.memory_type
        );
    }

    /* Reserve kernel code and data area to avoid using this area */
    for section in multiboot_information.elf_info.clone() {
        if section.should_allocate() && section.align_size() == PAGE_SIZE_USIZE {
            physical_memory_manager.reserve_memory(
                PAddress::new(section.address()),
                MSize::new(section.size()),
                MOrder::new(PAGE_SHIFT),
            );
        }
    }
    /* reserve Multiboot Information area */
    physical_memory_manager.reserve_memory(
        PAddress::new(multiboot_information.address),
        MSize::new(multiboot_information.size),
        0.into(),
    );

    /* TEMP: reserve boot code area for application processors */
    assert!(physical_memory_manager.reserve_memory(0.into(), PAGE_SIZE, 0.into()));

    /* Reserve Multiboot modules area */
    for e in multiboot_information.modules.iter() {
        if e.start_address != 0 && e.end_address != 0 {
            physical_memory_manager.reserve_memory(
                e.start_address.into(),
                (e.end_address - e.start_address).into(),
                0.into(),
            );
        }
    }

    /* Set up Virtual Memory Manager */
    let mut virtual_memory_manager = VirtualMemoryManager::new();
    virtual_memory_manager.init(true, &mut physical_memory_manager);
    for section in multiboot_information.elf_info.clone() {
        if !section.should_allocate() || section.align_size() != PAGE_SIZE_USIZE {
            continue;
        }
        assert_eq!(!PAGE_MASK & section.address(), 0);
        let aligned_size = MSize::new((section.size() - 1) & PAGE_MASK) + PAGE_SIZE;
        let permission = MemoryPermissionFlags::new(
            true,
            section.should_writable(),
            section.should_excusable(),
            false,
        );
        /* 初期化の段階で1 << order 分のメモリ管理を行ってはいけない。他の領域と重なる可能性がある。*/
        match virtual_memory_manager.map_address(
            PAddress::new(section.address()),
            Some(VAddress::new(section.address())),
            aligned_size,
            permission,
            MemoryOptionFlags::KERNEL,
            &mut physical_memory_manager,
        ) {
            Ok(address) => {
                if address == VAddress::new(section.address()) {
                    continue;
                }
                pr_err!(
                    "Virtual Address is different from Physical Address: V:{:#X} P:{:#X}",
                    address.to_usize(),
                    section.address()
                );
            }
            Err(e) => {
                pr_err!("Mapping ELF Section was failed: {:?}", e);
            }
        };
        panic!("Cannot map virtual memory correctly.");
    }
    /* TEMP: associate boot code area for application processors */
    virtual_memory_manager
        .map_address(
            PAddress::new(0),
            Some(VAddress::new(0)),
            PAGE_SIZE,
            MemoryPermissionFlags::data(),
            MemoryOptionFlags::MEMORY_MAP,
            &mut physical_memory_manager,
        )
        .expect("Cannot associate memory for boot code of Application Processors.");
    let aligned_multiboot = MemoryManager::page_align(
        PAddress::new(multiboot_information.address),
        MSize::new(multiboot_information.size),
    );
    let mapped_multiboot_address_base = virtual_memory_manager
        .map_address(
            aligned_multiboot.0,
            None,
            aligned_multiboot.1,
            MemoryPermissionFlags::rodata(),
            MemoryOptionFlags::MEMORY_MAP | MemoryOptionFlags::DO_NOT_FREE_PHYSICAL_ADDRESS,
            &mut physical_memory_manager,
        )
        .expect("Cannot map multiboot information");

    /* Set up Memory Manager */
    get_kernel_manager_cluster().system_memory_manager =
        SystemMemoryManager::new(physical_memory_manager);
    let mut memory_manager = get_kernel_manager_cluster()
        .system_memory_manager
        .create_new_memory_manager(virtual_memory_manager);

    /* Apply paging */
    memory_manager.set_paging_table();

    /* Set up Kernel Memory Alloc Manager */
    let mut object_allocator = ObjectAllocator::new();
    object_allocator.init(&mut memory_manager);

    /* Move Multiboot Information to allocated memory area */
    let mutex_memory_manager = Mutex::new(memory_manager);
    let new_mbi_address = object_allocator
        .alloc(
            MSize::new(multiboot_information.size),
            &mutex_memory_manager,
        )
        .expect("Cannot alloc memory for Multiboot Information.");
    unsafe {
        core::ptr::copy_nonoverlapping(
            (mapped_multiboot_address_base.to_usize()
                + (aligned_multiboot.0.to_usize() - multiboot_information.address))
                as *const u8,
            new_mbi_address.to_usize() as *mut u8,
            multiboot_information.size,
        )
    };
    /* Free old MultiBootInformation area */
    mutex_memory_manager
        .lock()
        .unwrap()
        .free(mapped_multiboot_address_base)
        .expect("Cannot free the map of multiboot information.");
    mutex_memory_manager.lock().unwrap().free_physical_memory(
        multiboot_information.address.into(),
        multiboot_information.size.into(),
    ); /* It may be already freed */

    /* Store managers to cluster */
    get_kernel_manager_cluster().memory_manager = mutex_memory_manager;
    get_cpu_manager_cluster().object_allocator = Mutex::new(object_allocator);
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

    /* Load font */
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
                    MemoryOptionFlags::PRE_RESERVED | MemoryOptionFlags::MEMORY_MAP,
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
                break;
            } else {
                pr_err!("mapping font data was failed: {:?}", vm_address.err());
            }
        }
    }
}
