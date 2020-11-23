/*
 * Global Allocator
 * allocator for core::alloc::GlobalAlloc
 */

use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};

use crate::arch::target_arch::paging::PAGE_SHIFT;

use crate::kernel::memory_manager::data_type::MSize;
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
        match get_cpu_manager_cluster()
            .object_allocator
            .lock()
            .unwrap()
            .alloc(layout_to_size(layout), memory_manager)
        {
            Ok(address) => address.into(),
            Err(e) => panic!("Cannot alloc memory for {:?}, Error: {:?}", layout, e),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let memory_manager = &get_kernel_manager_cluster().memory_manager;
        assert_eq!(layout.align() >> PAGE_SHIFT, 0);
        if let Err(e) = get_cpu_manager_cluster()
            .object_allocator
            .lock()
            .unwrap()
            .dealloc(
                (ptr as usize).into(),
                layout_to_size(layout),
                memory_manager,
            )
        {
            pr_err!("{:?}", e);
        }
    }
}

#[inline(always)]
fn layout_to_size(layout: Layout) -> MSize {
    core::cmp::max(layout.size(), layout.align()).into()
}
