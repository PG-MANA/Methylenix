//!
//! Cache Allocator
//!
//! This allocator is used to allocate specific object.
//!

use crate::arch::target_arch::paging::PAGE_SIZE_USIZE;

use crate::kernel::memory_manager::data_type::{Address, MPageOrder, MSize, VAddress};
use crate::kernel::memory_manager::{
    pool_allocator::PoolAllocator, MemoryError, MemoryManager, MemoryPermissionFlags,
};
use crate::kernel::sync::spin_lock::Mutex;

pub struct CacheAllocator<T> {
    allocator: PoolAllocator<T>,
    cache_threshold: usize,
}

impl<T> CacheAllocator<T> {
    pub const fn new(align: usize) -> Self {
        Self {
            allocator: PoolAllocator::new_with_align(align),
            cache_threshold: 0,
        }
    }

    pub fn init(
        &mut self,
        least_cache_entries: usize,
        memory_manager: &mut MemoryManager,
    ) -> Result<(), MemoryError> {
        self.cache_threshold = least_cache_entries;
        let page = memory_manager.alloc_pages(MPageOrder::new(0), MemoryPermissionFlags::data())?;
        unsafe {
            self.allocator
                .set_initial_pool(page.to_usize(), PAGE_SIZE_USIZE)
        };
        Ok(())
    }

    pub fn alloc(
        &mut self,
        memory_manager: Option<&Mutex<MemoryManager>>,
    ) -> Result<&'static mut T, MemoryError> {
        let result = self.allocator.alloc();
        if result.is_err() {
            if memory_manager.is_none() {
                return Err(MemoryError::AddressNotAvailable);
            }
            if let Ok(mut memory_manager) = memory_manager.unwrap().try_lock() {
                self.add_pool(&mut memory_manager)?;
                return if let Ok(a) = self.allocator.alloc() {
                    Ok(a)
                } else {
                    Err(MemoryError::AddressNotAvailable)
                };
            }
        }
        if memory_manager.is_some() && self.allocator.get_count() < self.cache_threshold {
            if let Ok(mut memory_manager) = memory_manager.unwrap().try_lock() {
                self.add_pool(&mut memory_manager)?;
            }
        }
        return Ok(result.unwrap());
    }

    pub fn free(&mut self, entry: &'static mut T) {
        self.allocator.free(entry);
    }

    pub fn add_free_area(&mut self, address: VAddress, size: MSize) {
        unsafe {
            self.allocator.add_pool(address.to_usize(), size.to_usize());
        }
    }

    pub fn add_pool(&mut self, memory_manager: &mut MemoryManager) -> Result<(), MemoryError> {
        let num_of_pages = MSize::new(self.cache_threshold * core::mem::size_of::<T>())
            .to_order(None)
            .to_page_order();
        let page =
            memory_manager.alloc_nonlinear_pages(num_of_pages, MemoryPermissionFlags::data())?;
        self.add_free_area(page, num_of_pages.to_offset());
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.allocator.get_count()
    }
}
