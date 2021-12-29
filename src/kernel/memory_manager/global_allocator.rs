//!
//! Global Allocator
//!
//! This is the allocator for core::alloc::GlobalAlloc

use crate::kernel::manager_cluster::get_cpu_manager_cluster;
use crate::kernel::memory_manager::data_type::{Address, MSize, VAddress};

use core::alloc::{GlobalAlloc, Layout};

pub struct GlobalAllocator;

#[global_allocator]
static GLOBAL_ALLOCATOR: GlobalAllocator = GlobalAllocator::new();

impl GlobalAllocator {
    pub const fn new() -> Self {
        Self {}
    }
}

#[alloc_error_handler]
fn alloc_error_oom(layout: Layout) -> ! {
    panic!("Memory Allocation({:?}) was failed.", layout);
}

unsafe impl GlobalAlloc for GlobalAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        match get_cpu_manager_cluster()
            .memory_allocator
            .kmalloc(layout_to_size(layout))
        {
            Ok(address) => address.to_usize() as *mut u8,
            Err(e) => {
                pr_err!("Cannot alloc memory for {:?}. Error: {:?}", layout, e);
                0 as *mut u8
            }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if let Err(e) = get_cpu_manager_cluster()
            .memory_allocator
            .kfree(VAddress::new(ptr as usize), layout_to_size(layout))
        {
            pr_err!("Cannot dealloc memory for {:?}. Error: {:?}", layout, e);
        }
    }
}

#[inline(always)]
fn layout_to_size(layout: Layout) -> MSize {
    core::cmp::max(layout.size(), layout.align()).into()
}
