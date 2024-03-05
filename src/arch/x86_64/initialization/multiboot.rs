//!
//! Init codes for device, memory and else based on MultibootInformation
//!
//! This module is called by boot function.

use super::MEMORY_FOR_PHYSICAL_MEMORY_MANAGER;

use crate::arch::target_arch::{
    context::memory_layout::{kernel_area_to_physical_address, KERNEL_MAP_START_ADDRESS},
    paging::{PAGE_MASK, PAGE_SHIFT, PAGE_SIZE, PAGE_SIZE_USIZE},
};

use crate::kernel::{
    collections::init_struct,
    drivers::{efi::memory_map::EfiMemoryType, multiboot::MultiBootInformation},
    graphic_manager::font::FontType,
    manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster},
    memory_manager::{
        data_type::{
            Address, MOrder, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress, VAddress,
        },
        io_remap,
        memory_allocator::MemoryAllocator,
        physical_memory_manager::PhysicalMemoryManager,
        system_memory_manager::get_physical_memory_manager,
        system_memory_manager::SystemMemoryManager,
        virtual_memory_manager::VirtualMemoryManager,
        MemoryManager,
    },
};

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
            core::ptr::addr_of!(MEMORY_FOR_PHYSICAL_MEMORY_MANAGER) as usize,
            mem::size_of_val(&*core::ptr::addr_of!(MEMORY_FOR_PHYSICAL_MEMORY_MANAGER)),
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
            EfiMemoryType::EfiReservedMemoryType |
            EfiMemoryType::EfiBootServicesData/* for BGRT */ |
            EfiMemoryType::EfiRuntimeServicesCode |
            EfiMemoryType::EfiRuntimeServicesData |
            EfiMemoryType::EfiUnusableMemory |
            EfiMemoryType::EfiACPIReclaimMemory |
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
            _ => {}
        }
        pr_info!(
            "[{:#016X}~{:#016X}] {}",
            entry.physical_start,
            MSize::new((entry.number_of_pages as usize) << PAGE_SHIFT)
                .to_end_address(PAddress::new(entry.physical_start))
                .to_usize(),
            entry.memory_type
        );
    }

    /* Reserve kernel code and data area to avoid using this area */
    for section in multiboot_information.elf_info.clone() {
        if section.is_section_allocate() && ((section.get_address() as usize) & !PAGE_MASK) == 0 {
            let virtual_address = VAddress::new(section.get_address() as usize);
            let physical_address = if virtual_address >= KERNEL_MAP_START_ADDRESS {
                kernel_area_to_physical_address(virtual_address)
            } else {
                virtual_address.to_direct_mapped_p_address()
            };
            physical_memory_manager
                .reserve_memory(
                    physical_address,
                    MSize::new(section.get_size() as usize),
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
            MOrder::new(0),
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
                    PAddress::new(e.start_address),
                    MSize::new(e.end_address - e.start_address),
                    MOrder::new(0),
                )
                .expect("Failed to reserve memory area of the multiboot modules");
        }
    }

    /* Set up Virtual Memory Manager */
    let mut virtual_memory_manager = VirtualMemoryManager::new();
    virtual_memory_manager.init_system(
        PAddress::new(max_available_address),
        &mut physical_memory_manager,
    );
    init_struct!(
        get_kernel_manager_cluster().system_memory_manager,
        SystemMemoryManager::new(physical_memory_manager)
    );
    get_kernel_manager_cluster()
        .system_memory_manager
        .init_pools(&mut virtual_memory_manager);

    for section in multiboot_information.elf_info.clone() {
        let section_address = section.get_address() as usize;
        if !section.is_section_allocate() || (section_address & !PAGE_MASK) != 0 {
            continue;
        }
        let aligned_size = MSize::new((section.get_size() as usize - 1) & PAGE_MASK) + PAGE_SIZE;
        let permission = MemoryPermissionFlags::new(
            true,
            section.is_section_writable(),
            section.is_section_excusable(),
            false,
        );
        let virtual_address = VAddress::new(section_address);
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
            get_physical_memory_manager(),
        ) {
            Ok(address) => {
                if address == VAddress::new(section_address) {
                    continue;
                }
                pr_err!(
                    "Virtual Address is different from Physical Address: V:{:#X} P:{:#X}",
                    address.to_usize(),
                    section_address
                );
            }
            Err(e) => {
                pr_err!("Mapping ELF Section was failed: {:?}", e);
            }
        };
        panic!("Cannot map virtual memory correctly.");
    }

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
            MemoryOptionFlags::IO_MAP | MemoryOptionFlags::DO_NOT_FREE_PHYSICAL_ADDRESS,
            get_physical_memory_manager(),
        )
        .expect("Cannot map multiboot information");

    /* Set up Memory Manager */
    init_struct!(
        get_kernel_manager_cluster().kernel_memory_manager,
        MemoryManager::new(virtual_memory_manager)
    );
    /* Apply paging */
    get_kernel_manager_cluster()
        .kernel_memory_manager
        .set_paging_table();

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
        .kernel_memory_manager
        .free(mapped_multiboot_address_base)
        .expect("Cannot free the map of multiboot information.");
    let _ = get_kernel_manager_cluster()
        .kernel_memory_manager
        .free_physical_memory(
            PAddress::new(multiboot_information.address),
            MSize::new(multiboot_information.size),
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
            let vm_address = io_remap!(
                PAddress::new(module.start_address),
                MSize::new(module.end_address - module.start_address),
                MemoryPermissionFlags::rodata(),
                MemoryOptionFlags::PRE_RESERVED
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
