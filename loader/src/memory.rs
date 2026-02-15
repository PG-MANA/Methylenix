//!
//! Simple Memory Allocator
//!
//! Supported: Baremetal(DTB), UEFI

use crate::arch::target_arch::paging;

use crate::kernel::drivers::efi::memory_map::{
    EfiMemoryAttribute, EfiMemoryDescriptor, EfiMemoryType,
};

const fn create_available_memory(base_address: usize, size: usize) -> EfiMemoryDescriptor {
    EfiMemoryDescriptor {
        memory_type: EfiMemoryType::EfiConventionalMemory,
        physical_start: base_address,
        virtual_start: base_address,
        number_of_pages: (size >> paging::PAGE_SHIFT) as u64, /* round down */
        attribute: EfiMemoryAttribute::EfiMemoryWb,
    }
}

const fn create_allocated_memory(
    base_address: usize,
    number_of_pages: usize,
) -> EfiMemoryDescriptor {
    EfiMemoryDescriptor {
        memory_type: EfiMemoryType::EfiLoaderData,
        physical_start: base_address,
        virtual_start: base_address,
        number_of_pages: number_of_pages as u64,
        attribute: EfiMemoryAttribute::EfiMemoryWb,
    }
}

static mut MEMORY_LIST: [EfiMemoryDescriptor; 64] = [EfiMemoryDescriptor {
    memory_type: EfiMemoryType::EfiMaxMemoryType,
    physical_start: 0,
    virtual_start: 0,
    number_of_pages: 0,
    attribute: EfiMemoryAttribute::EfiMemoryUc,
}; 64];

pub fn allocate_pages(pages: usize) -> Option<usize> {
    let list = unsafe { (&raw mut MEMORY_LIST).as_mut().unwrap() };
    let e = list.iter_mut().find(|e| {
        e.memory_type == EfiMemoryType::EfiConventionalMemory && e.number_of_pages >= pages as u64
    })?;
    let page = e.physical_start;
    e.physical_start += pages << paging::PAGE_SHIFT;
    e.virtual_start += pages << paging::PAGE_SHIFT;
    e.number_of_pages -= pages as u64;

    for e in list.iter_mut() {
        if e.memory_type == EfiMemoryType::EfiMaxMemoryType {
            *e = create_allocated_memory(page, pages);
            return Some(page);
        }
        if e.memory_type != EfiMemoryType::EfiLoaderData {
            continue;
        }
        if e.physical_start + (e.number_of_pages << paging::PAGE_SHIFT) as usize == page {
            e.number_of_pages += pages as u64;
            return Some(page);
        } else if page + (pages << paging::PAGE_SHIFT) == e.physical_start {
            e.physical_start = page;
            e.virtual_start = page;
            e.number_of_pages += pages as u64;
            return Some(page);
        }
    }
    panic!("Failed to insert the entry");
}

fn reserve_memory(start: usize, size: usize, memory_type: EfiMemoryType) {
    if size == 0 {
        return;
    }
    let end = ((start + size - 1) & paging::PAGE_MASK) + paging::PAGE_SIZE_USIZE;
    let start = start & paging::PAGE_MASK;
    let size = end - start;
    assert_eq!(size & !paging::PAGE_MASK, 0);
    let list = unsafe { (&raw mut MEMORY_LIST).as_mut().unwrap() };

    let mut new_start = 0;
    let mut new_size = 0;

    for e in list.iter_mut() {
        if e.memory_type != EfiMemoryType::EfiConventionalMemory {
            continue;
        }
        let entry_start = e.physical_start;
        let entry_end = entry_start + (e.number_of_pages << paging::PAGE_SHIFT) as usize;
        let entry_range = entry_start..entry_end;

        if (start..end).contains(&entry_start) && (start..end).contains(&entry_end) {
            e.number_of_pages = 0;
            e.memory_type = EfiMemoryType::EfiMaxMemoryType;
            break;
        } else {
            let mut contained = false;
            if entry_range.contains(&start) {
                e.number_of_pages = ((start - entry_start) >> paging::PAGE_SHIFT) as u64;
                contained = true;
            }
            if entry_range.contains(&end) {
                if entry_start < start {
                    // [entry_start..|start..end|..entry_end]
                    assert!(contained);
                    new_start = end;
                    new_size = entry_end - end;
                } else {
                    e.physical_start = end;
                    e.virtual_start = end;
                    e.number_of_pages = ((entry_end - end) >> paging::PAGE_SHIFT) as u64;
                }
                contained = true;
            }
            if contained {
                break;
            }
        }
    }

    if new_start != 0 {
        assert_ne!(new_size, 0);
        list.iter_mut()
            .find(|e| e.memory_type == EfiMemoryType::EfiMaxMemoryType)
            .map(|e| *e = create_available_memory(new_start, new_size))
            .expect("Failed to insert the entry");
    }

    list.iter_mut()
        .find(|e| e.memory_type == EfiMemoryType::EfiMaxMemoryType)
        .map(|e| {
            *e = EfiMemoryDescriptor {
                memory_type,
                physical_start: start,
                virtual_start: start,
                number_of_pages: (((size - 1) >> paging::PAGE_SHIFT) + 1) as u64,
                attribute: EfiMemoryAttribute::EfiMemoryUc,
            }
        })
        .expect("Failed to insert the entry");
}

pub fn init_memory_allocator(dtb: &crate::dtb::DtbManager, loader_area: &[(usize, usize)]) {
    let memory = dtb
        .search_node(b"memory", None)
        .expect("Failed to get memory node");
    let list = unsafe { (&raw mut MEMORY_LIST).as_mut().unwrap() };

    let mut i = 0;
    while let Some((base, size)) = dtb.read_reg_property(&memory, i) {
        println!("RAM:\t\t[{base:#18X} ~ {:#18X}]", base + size);
        if i < list.len() {
            let number_of_pages = (size >> paging::PAGE_SHIFT) as u64;
            list[i] = EfiMemoryDescriptor {
                memory_type: EfiMemoryType::EfiConventionalMemory,
                physical_start: base,
                virtual_start: base,
                number_of_pages,
                attribute: EfiMemoryAttribute::EfiMemoryWb,
            }
        } else {
            println!("Too many memory area, drop the area information");
        }
        i += 1;
    }

    if let Some(reserved) = dtb.search_node(b"reserved-memory", None)
        && let Some(sub) = dtb.iterate_sub_node(&reserved)
    {
        for n in sub {
            i = 0;
            while let Some((base, size)) = dtb.read_reg_property(&n, i) {
                println!("Reserved:\t[{base:#18X} ~ {:#18X}]", base + size);
                reserve_memory(base, size, EfiMemoryType::EfiUnusableMemory);
                i += 1;
            }
        }
    }

    for e in loader_area {
        println!("Reserved:\t[{:#18X} ~ {:#18X}]", e.0, e.0 + e.1);
        reserve_memory(e.0, e.1, EfiMemoryType::EfiLoaderData);
    }
}

pub fn store_memory_map(memory_map: &mut [EfiMemoryDescriptor; 64]) {
    let list = unsafe { (&raw mut MEMORY_LIST).as_mut().unwrap() };

    /* Sort entries by the address and Merge continuous entries */
    list.sort_unstable_by_key(|a| a.physical_start);
    loop {
        let mut merged = false;
        for i in 0..(list.len() - 1) {
            let (x, y) = list.split_at_mut(i + 1);
            let a = x.last_mut().unwrap();
            let Some(b) = y
                .iter_mut()
                .find(|e| e.memory_type != EfiMemoryType::EfiMaxMemoryType)
            else {
                break;
            };
            if a.memory_type == b.memory_type
                && a.attribute == b.attribute
                && (a.physical_start + (a.number_of_pages << paging::PAGE_SHIFT) as usize)
                    == b.physical_start
            {
                a.number_of_pages += b.number_of_pages;
                *b = EfiMemoryDescriptor {
                    memory_type: EfiMemoryType::EfiMaxMemoryType,
                    physical_start: 0,
                    virtual_start: 0,
                    number_of_pages: 0,
                    attribute: EfiMemoryAttribute::EfiMemoryWb,
                };
                merged = true;
            }
        }
        if !merged {
            break;
        }
    }

    let mut i = 0;
    for e in list.iter() {
        if e.memory_type != EfiMemoryType::EfiMaxMemoryType && i < memory_map.len() {
            memory_map[i] = *e;
            i += 1;
        }
    }
    memory_map[i..].fill(EfiMemoryDescriptor {
        memory_type: EfiMemoryType::EfiMaxMemoryType,
        physical_start: 0,
        virtual_start: 0,
        number_of_pages: 0,
        attribute: EfiMemoryAttribute::EfiMemoryUc,
    });
}
