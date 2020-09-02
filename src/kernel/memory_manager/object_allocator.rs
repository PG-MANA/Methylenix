//!
//! Object Allocator
//!
//! This is the front end of memory management system.
//! The Object allocator is used when the system needs to allocate small object which will be freed soon.
//!

mod heap_allocator;

use self::heap_allocator::HeapAllocator;

use crate::arch::target_arch::paging::PAGE_SIZE;

use crate::kernel::memory_manager::data_type::{MSize, VAddress};
use crate::kernel::memory_manager::pool_allocator::PoolAllocator;
use crate::kernel::memory_manager::{MemoryError, MemoryManager, MemoryPermissionFlags};
use crate::kernel::sync::spin_lock::Mutex;

struct CacheAllocator {
    object_size: usize,
    heap: usize,
    align_order: usize,
    available_count: usize,
}

pub struct ObjectAllocator {
    heap_allocator: HeapAllocator,
    cache_allocator: PoolAllocator<CacheAllocator>,
}

impl ObjectAllocator {
    pub const fn new() -> Self {
        ObjectAllocator {
            heap_allocator: HeapAllocator::new(),
            cache_allocator: PoolAllocator::new(),
        }
    }

    pub fn init(&mut self, m_manager: &mut MemoryManager) -> bool {
        if let Err(e) = self.heap_allocator.init(m_manager) {
            pr_err!("Setting up ObjectAllocator is failed: {:?}", e);
            return false;
        }
        /* TODO: init cache_allocator */
        return true;
    }

    pub fn alloc(
        &mut self,
        size: MSize,
        memory_manager: &Mutex<MemoryManager>,
    ) -> Result<VAddress, MemoryError> {
        if size.is_zero() {
            Err(MemoryError::InvalidSize)
        } else if size > PAGE_SIZE {
            memory_manager.lock().unwrap().alloc_pages(
                size.to_order(None).to_page_order(),
                MemoryPermissionFlags::data(),
            )
        } else {
            self.heap_allocator.alloc(size, memory_manager)
        }
    }

    pub fn dealloc(
        &mut self,
        address: VAddress,
        size: MSize,
        memory_manager: &Mutex<MemoryManager>,
    ) -> Result<(), MemoryError> {
        if size.is_zero() {
            Err(MemoryError::InvalidSize)
        } else if size > PAGE_SIZE {
            memory_manager.lock().unwrap().free(address)
        } else {
            self.heap_allocator.dealloc(address, size);
            Ok(())
        }
    }
}
