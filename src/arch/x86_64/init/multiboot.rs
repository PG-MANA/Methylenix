//!
//! Init codes for device, memory and else based on MultibootInformation
//!
//! This module is called by boot function.

use super::MEMORY_FOR_PHYSICAL_MEMORY_MANAGER;

use crate::arch::target_arch::context::memory_layout::{
    kernel_area_to_physical_address, KERNEL_MAP_START_ADDRESS,
};
use crate::arch::target_arch::paging::{PAGE_MASK, PAGE_SHIFT, PAGE_SIZE, PAGE_SIZE_USIZE};

use crate::kernel::drivers::efi::boot_service::memory_map::EfiMemoryType;
use crate::kernel::drivers::multiboot::MultiBootInformation;
use crate::kernel::graphic_manager::font::FontType;
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::data_type::{Address, MOrder, MSize, PAddress, VAddress};
use crate::kernel::memory_manager::physical_memory_manager::PhysicalMemoryManager;
use crate::kernel::memory_manager::virtual_memory_manager::VirtualMemoryManager;
use crate::kernel::memory_manager::{
    data_type::MemoryOptionFlags, data_type::MemoryPermissionFlags, MemoryManager,
    SystemMemoryManager,
};

use crate::kernel::memory_manager::memory_allocator::MemoryAllocator;
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
        physical_memory_manager.add_memory_entry_pool(
            &MEMORY_FOR_PHYSICAL_MEMORY_MANAGER as *const _ as usize,
            mem::size_of_val(&MEMORY_FOR_PHYSICAL_MEMORY_MANAGER),
        );
    }
    let mut max_available_address = 0;
    for entry in multiboot_information.memory_map_info.clone() {
        if entry.m_type == 1 {
            /* Available memory */
            physical_memory_manager
                .free(
                    PAddress::new(entry.addr as usize),
                    MSize::new(entry.length as usize),
                    true,
                )
                .expect("Failed to free available memory");
            if max_available_address < (entry.addr + entry.length) as usize {
                max_available_address =
                    ((((entry.addr + entry.length) as usize) - 1) & PAGE_MASK) + PAGE_SIZE_USIZE;
            }
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
                if let Err(e) =
                   physical_memory_manager.reserve_memory(
                       PAddress::new(entry.physical_start),
                       MSize::new((entry.number_of_pages as usize) << PAGE_SHIFT),
                       MOrder::new(0)) {
                    pr_warn!("Failed to free {:?}: {:?}", entry.memory_type, e);
                }
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
        if section.should_allocate() && (section.address() & !PAGE_MASK) == 0 {
            let virtual_address = VAddress::new(section.address());
            let physical_address = if virtual_address >= KERNEL_MAP_START_ADDRESS {
                kernel_area_to_physical_address(virtual_address)
            } else {
                virtual_address.to_direct_mapped_p_address()
            };
            physical_memory_manager
                .reserve_memory(
                    physical_address,
                    MSize::new(section.size()),
                    MOrder::new(PAGE_SHIFT),
                )
                .expect("Failed to reserve memory");
        }
    }
    /* reserve Multiboot Information area */
    physical_memory_manager
        .reserve_memory(
            PAddress::new(multiboot_information.address),
            MSize::new(multiboot_information.size),
            0.into(),
        )
        .expect("Failed to reserve memory area of the multiboot information");

    /* TEMP: reserve boot code area for application processors */
    physical_memory_manager
        .reserve_memory(PAddress::new(0), PAGE_SIZE, MOrder::new(0))
        .expect("Failed to reserve boot code area");

    /* Reserve Multiboot modules area */
    for e in multiboot_information.modules.iter() {
        if e.start_address != 0 && e.end_address != 0 {
            physical_memory_manager
                .reserve_memory(
                    e.start_address.into(),
                    (e.end_address - e.start_address).into(),
                    0.into(),
                )
                .expect("Failed to reserve memory area of the multiboot modules");
        }
    }

    /* Set up Virtual Memory Manager */
    let mut virtual_memory_manager = VirtualMemoryManager::new();
    virtual_memory_manager.init(
        true,
        PAddress::new(max_available_address),
        &mut physical_memory_manager,
    );
    for section in multiboot_information.elf_info.clone() {
        if !section.should_allocate() || (section.address() & !PAGE_MASK) != 0 {
            continue;
        }
        let aligned_size = MSize::new((section.size() - 1) & PAGE_MASK) + PAGE_SIZE;
        let permission = MemoryPermissionFlags::new(
            true,
            section.should_writable(),
            section.should_excusable(),
            false,
        );
        let virtual_address = VAddress::new(section.address());
        let physical_address = if virtual_address >= KERNEL_MAP_START_ADDRESS {
            kernel_area_to_physical_address(virtual_address)
        } else {
            virtual_address.to_direct_mapped_p_address()
        };
        /* 初期化の段階で1 << order 分のメモリ管理を行ってはいけない。他の領域と重なる可能性がある。*/
        match virtual_memory_manager.map_address(
            physical_address,
            Some(virtual_address),
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
    get_kernel_manager_cluster().memory_manager = memory_manager;

    /* Set up Kernel Memory Alloc Manager */
    let mut memory_allocator = MemoryAllocator::new();
    memory_allocator
        .init()
        .expect("Failed to init MemoryAllocator");

    /* Move Multiboot Information to allocated memory area */
    let new_mbi_address = memory_allocator
        .kmalloc(MSize::new(multiboot_information.size))
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
    get_cpu_manager_cluster().memory_allocator = memory_allocator;
    /* Free old MultiBootInformation area */
    get_kernel_manager_cluster()
        .memory_manager
        .free(mapped_multiboot_address_base)
        .expect("Cannot free the map of multiboot information.");
    let _ = get_kernel_manager_cluster()
        .memory_manager
        .free_physical_memory(
            multiboot_information.address.into(),
            multiboot_information.size.into(),
        ); /* It may be already freed */

    /* Store managers to cluster */
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
            let vm_address = get_kernel_manager_cluster().memory_manager.mmap(
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
