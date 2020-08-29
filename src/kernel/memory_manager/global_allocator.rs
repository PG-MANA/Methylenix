/*
 * Global Allocator
 * allocator for core::alloc::GlobalAlloc
 */

use crate::kernel::manager_cluster::get_kernel_manager_cluster;

use crate::arch::target_arch::paging::PAGE_SHIFT;

use crate::kernel::memory_manager::data_type::Address;
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
    panic!("Memory Allocation Error Err:{:?}", layout);
}

unsafe impl GlobalAlloc for GlobalAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let memory_manager = &get_kernel_manager_cluster().memory_manager;
        assert_eq!(layout.align() >> PAGE_SHIFT, 0);
        if let Some(address) = get_kernel_manager_cluster()
            .kernel_memory_alloc_manager
            .lock()
            .unwrap()
            .vmalloc(layout.size().into(), layout.align().into(), memory_manager)
        {
            address.to_usize() as *mut u8
        } else {
            panic!("Cannot alloc memory for {:?}", layout);
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let memory_manager = &get_kernel_manager_cluster().memory_manager;
        assert_eq!(layout.align() >> PAGE_SHIFT, 0);
        get_kernel_manager_cluster()
            .kernel_memory_alloc_manager
            .lock()
            .unwrap()
            .vfree((ptr as usize).into(), memory_manager)
    }
}
