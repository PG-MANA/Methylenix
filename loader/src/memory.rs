//!
//! Simple Memory Allocator
//!
//! Supported: Baremetal(DTB), UEFI

use crate::arch::target_arch::paging;

use crate::kernel::drivers::boot_information::RamMapEntry;

static mut MEMORY_LIST: [(usize, usize); 64] = [(0, 0); 64];

pub fn allocate_pages(pages: usize) -> Option<usize> {
    let alloc_size = pages << paging::PAGE_SHIFT;
    for e in unsafe { (&raw mut MEMORY_LIST).as_mut().unwrap().iter_mut() } {
        if e.0 == 0 || e.1 < alloc_size {
            continue;
        }
        let page = e.0;
        e.0 += alloc_size;
        e.1 -= alloc_size;
        return Some(page);
    }
    None
}

fn reserve_memory(start: usize, size: usize) {
    let end = start + size;
    let list = unsafe { (&raw mut MEMORY_LIST).as_mut().unwrap() };

    for i in 0..list.len() {
        let e = &mut list[i];
        if e.0 == 0 || e.1 == 0 {
            continue;
        }
        let entry_start = e.0;
        let entry_end = entry_start + e.1;
        let entry_range = entry_start..entry_end;
        let mut new_start = 0;
        let mut new_size = 0;

        if entry_range.contains(&start) {
            e.1 = start - entry_start;
        }
        if entry_range.contains(&end) {
            if entry_start < start {
                // [entry_start..|start..end|..entry_end]
                assert_eq!(e.1, start - entry_start);
                new_start = end;
                new_size = entry_end - end;
            } else {
                e.0 = end;
                e.1 = entry_end - end;
            }
        } else if (start..end).contains(&entry_start) && (start..end).contains(&entry_end) {
            e.1 = 0;
        }
        if e.1 == 0 {
            e.0 = 0;
        }

        if new_start != 0 {
            assert_ne!(new_size, 0);
            list[i..]
                .iter_mut()
                .find(|e| e.0 == 0 && e.1 == 0)
                .map(|e| {
                    e.0 = new_start;
                    e.1 = new_size;
                })
                .expect("Failed to insert the entry");
        }
    }
}

pub fn init_memory_allocator(
    dtb: &crate::dtb::DtbManager,
    loader_area: &[(usize, usize)],
    ram_map: &mut [RamMapEntry],
) {
    let memory = dtb
        .search_node(b"memory", None)
        .expect("Failed to get memory node");

    let mut i = 0;
    while let Some((base, size)) = dtb.read_reg_property(&memory, i) {
        let end = base + size;
        println!("RAM:\t\t[{base:#18X} ~ {end:#18X}]");
        unsafe { MEMORY_LIST[i] = (base, end - base) };
        i += 1;
    }

    if let Some(reserved) = dtb.search_node(b"reserved-memory", None)
        && let Some(sub) = dtb.iterate_sub_node(&reserved)
    {
        for n in sub {
            i = 0;
            while let Some((base, size)) = dtb.read_reg_property(&n, i) {
                println!("Reserved:\t[{base:#18X} ~ {:#18X}]", base + size);
                reserve_memory(base, size);
                i += 1;
            }
        }
    }

    for e in loader_area {
        reserve_memory(e.0, e.1);
    }

    let mut i = 0;
    for (b, s) in unsafe { (&raw const MEMORY_LIST).as_ref().unwrap().iter() } {
        if *b == 0 || *s == 0 {
            continue;
        }
        println!("Allocatable:\t[{b:#18X} ~ {:#18X}]", b + s);

        if i < ram_map.len() {
            ram_map[i] = RamMapEntry { base: *b, size: *s };
            i += 1;
        } else {
            pr_err!("Too many RAM area");
        }
    }
}
