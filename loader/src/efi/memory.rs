//!
//! Memory Allocator with UEFI
//!

use super::BOOT_SERVICES;

use crate::arch::target_arch::context::memory_layout::{
    direct_map_to_physical_address_loader, physical_address_to_direct_map_loader,
};

use crate::kernel::drivers::boot_information::{
    AdditonalMemoryMap, BootInformation, BootInformationMemoryMap,
};
use crate::kernel::drivers::efi::{EFI_PAGE_SIZE, EfiBootServices, EfiStatus, memory_map};
use crate::kernel::memory_manager::data_type::{Address, MSize, PAddress};

use core::num::NonZeroUsize;

type Descriptor = memory_map::EfiMemoryDescriptor;

const DESCRIPTOR_SIZE: usize = size_of::<Descriptor>();

pub fn allocate_pages(num_of_pages: usize) -> Option<usize> {
    let mut address: usize = 0;
    let result = (unsafe { &*BOOT_SERVICES }.allocate_pages)(
        memory_map::EfiAllocateType::AllocateAnyPages,
        memory_map::EfiMemoryType::LoaderData,
        num_of_pages,
        &mut address,
    );
    if result != EfiStatus::Success {
        println!("Failed to allocate memory: {result:?}");
        None
    } else {
        Some(address)
    }
}

pub fn store_memory_map_extra(
    boot_services: &EfiBootServices,
    boot_information: &mut BootInformation,
    mut memory_map_size: usize,
) -> usize {
    let memory_map: *mut Descriptor;
    let mut map_key: usize = 0;
    let mut descriptor_size: usize = 0;
    let mut descriptor_version: u32 = 0;

    if let Some(i) = &boot_information.additional_memory_map {
        memory_map = direct_map_to_physical_address_loader(i.address).to_usize() as *mut Descriptor;
        memory_map_size = i.allocated_size.to_usize();
    } else {
        assert_ne!(memory_map_size, 0);
        let buffer_pages = (memory_map_size / EFI_PAGE_SIZE) + 2;
        let buffer =
            allocate_pages(buffer_pages).expect("Failed to allocate memory for the memory map");
        memory_map_size = buffer_pages * EFI_PAGE_SIZE;
        memory_map = buffer as *mut _;
        let _ = boot_information
            .additional_memory_map
            .insert(AdditonalMemoryMap {
                address: physical_address_to_direct_map_loader(PAddress::new(buffer)),
                allocated_size: MSize::new(memory_map_size),
                actual_size: NonZeroUsize::new(memory_map_size).unwrap(), /* Set actual size later*/
            });
    }

    let r = (boot_services.get_memory_map)(
        &mut memory_map_size,
        memory_map,
        &mut map_key,
        &mut descriptor_size,
        &mut descriptor_version,
    );
    assert_eq!(r, EfiStatus::Success, "Failed to get memory map: {r:?}");
    assert!(descriptor_size >= DESCRIPTOR_SIZE);
    assert!(descriptor_version >= 1);
    assert_ne!(memory_map_size, 0);

    let number_of_entries = memory_map_size / descriptor_size;

    /* Adjust the copied memory map */
    if descriptor_size > DESCRIPTOR_SIZE {
        for i in 1..number_of_entries {
            unsafe {
                core::ptr::copy(
                    ((memory_map as usize) + (i * descriptor_size)) as *const Descriptor,
                    ((memory_map as usize) + (i * DESCRIPTOR_SIZE)) as *mut Descriptor,
                    1,
                )
            };
        }
    }

    /* Copy ConventionalMemory to [`BootInformation::memory_map`] */
    let mut i = 0;
    let map = unsafe { core::slice::from_raw_parts(memory_map, number_of_entries) };
    for e in map {
        if e.memory_type == memory_map::EfiMemoryType::ConventionalMemory {
            boot_information.memory_map[i] = *e;
            i += 1;
            if i == boot_information.memory_map.len() {
                break;
            }
        }
    }

    /* Copy other entries as possible */
    for e in map {
        if i == boot_information.memory_map.len() {
            break;
        }
        if e.memory_type != memory_map::EfiMemoryType::ConventionalMemory {
            boot_information.memory_map[i] = *e;
            i += 1;
        }
    }

    boot_information
        .memory_map
        .sort_unstable_by_key(|a| a.physical_start);
    boot_information
        .additional_memory_map
        .as_mut()
        .unwrap()
        .actual_size = NonZeroUsize::new(memory_map_size).unwrap();

    map_key
}

pub fn store_memory_map(
    boot_services: &EfiBootServices,
    boot_information: &mut BootInformation,
) -> usize {
    let mut map_key = 0;
    let mut memory_map_size = size_of::<BootInformationMemoryMap>();
    let mut descriptor_size: usize = 0;
    let mut descriptor_version: u32 = 0;

    if boot_information.additional_memory_map.is_some() {
        return store_memory_map_extra(boot_services, boot_information, 0);
    }

    let r = (boot_services.get_memory_map)(
        &mut memory_map_size,
        boot_information.memory_map.as_mut_ptr(),
        &mut map_key,
        &mut descriptor_size,
        &mut descriptor_version,
    );
    if r == EfiStatus::Success {
        assert!(descriptor_size >= DESCRIPTOR_SIZE);
        assert!(descriptor_version >= 1);
        assert_ne!(memory_map_size, 0);

        let number_of_entries = memory_map_size / descriptor_size;
        if descriptor_size > DESCRIPTOR_SIZE {
            for i in 1..number_of_entries {
                unsafe {
                    core::ptr::copy(
                        ((boot_information.memory_map.as_mut_ptr() as usize)
                            + (i * descriptor_size)) as *const Descriptor,
                        &mut boot_information.memory_map[i],
                        1,
                    )
                };
            }
            boot_information.memory_map[number_of_entries..].fill_with(Descriptor::default);
        }
        map_key
    } else if r == EfiStatus::BufferTooSmall {
        store_memory_map_extra(boot_services, boot_information, memory_map_size)
    } else {
        panic!("Failed to get memory map: {r:?}");
    }
}
