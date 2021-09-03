//!
//! Global Allocator
//!
//! This is the allocator for core::alloc::GlobalAlloc

use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::data_type::{Address, MSize};

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
        let memory_manager = &get_kernel_manager_cluster().memory_manager;
        match get_cpu_manager_cluster()
            .object_allocator
            .alloc(layout_to_size(layout), memory_manager)
        {
            Ok(address) => address.to_usize() as *mut u8,
            Err(e) => {
                pr_err!("Cannot alloc memory for {:?}. Error: {:?}", layout, e);
                0 as *mut u8
            }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let memory_manager = &get_kernel_manager_cluster().memory_manager;
        if let Err(e) = get_cpu_manager_cluster().object_allocator.dealloc(
            (ptr as usize).into(),
            layout_to_size(layout),
            memory_manager,
        ) {
            pr_err!("Cannot dealloc memory for {:?}. Error: {:?}", layout, e);
        }
    }
}

#[inline(always)]
fn layout_to_size(layout: Layout) -> MSize {
    core::cmp::max(layout.size(), layout.align()).into()
}
