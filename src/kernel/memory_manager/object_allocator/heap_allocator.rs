//!
//! Heap Allocator
//!

use crate::kernel::memory_manager::data_type::{Address, MPageOrder, MSize, VAddress};
use crate::kernel::memory_manager::{
    pool_allocator::PoolAllocator, MemoryError, MemoryManager, MemoryPermissionFlags,
};

pub struct HeapAllocator {
    slab_64: PoolAllocator<[u8; 64]>,
    slab_128: PoolAllocator<[u8; 128]>,
    slab_256: PoolAllocator<[u8; 256]>,
    slab_512: PoolAllocator<[u8; 512]>,
    slab_1024: PoolAllocator<[u8; 1024]>,
    slab_2048: PoolAllocator<[u8; 2048]>,
    slab_4096: PoolAllocator<[u8; 4096]>,
}

impl HeapAllocator {
    const DEFAULT_ALLOC_PAGE_ORDER: MPageOrder = MPageOrder::new(4);

    pub const fn new() -> Self {
        Self {
            slab_64: PoolAllocator::new(),
            slab_128: PoolAllocator::new(),
            slab_256: PoolAllocator::new(),
            slab_512: PoolAllocator::new(),
            slab_1024: PoolAllocator::new(),
            slab_2048: PoolAllocator::new(),
            slab_4096: PoolAllocator::new(),
        }
    }

    pub fn init(&mut self, memory_manager: &mut MemoryManager) -> Result<(), MemoryError> {
        macro_rules! alloc_and_set_pool {
            ($allocator:expr) => {{
                let address = memory_manager.alloc_pages(
                    Self::DEFAULT_ALLOC_PAGE_ORDER,
                    MemoryPermissionFlags::data(),
                )?;
                unsafe {
                    $allocator.set_initial_pool(
                        address.to_usize(),
                        Self::DEFAULT_ALLOC_PAGE_ORDER.to_offset().to_usize(),
                    )
                };
            }};
        }
        alloc_and_set_pool!(self.slab_64);
        alloc_and_set_pool!(self.slab_128);
        alloc_and_set_pool!(self.slab_256);
        alloc_and_set_pool!(self.slab_512);
        alloc_and_set_pool!(self.slab_1024);
        alloc_and_set_pool!(self.slab_2048);
        alloc_and_set_pool!(self.slab_4096);

        Ok(())
    }

    pub fn alloc(&mut self, size: MSize) -> Result<VAddress, ()> {
        if size <= MSize::new(64) {
            self.slab_64
                .alloc_ptr()
                .and_then(|a| Ok(VAddress::new(a as usize)))
        } else if size <= MSize::new(128) {
            self.slab_128
                .alloc_ptr()
                .and_then(|a| Ok(VAddress::new(a as usize)))
        } else if size <= MSize::new(256) {
            self.slab_256
                .alloc_ptr()
                .and_then(|a| Ok(VAddress::new(a as usize)))
        } else if size <= MSize::new(512) {
            self.slab_512
                .alloc_ptr()
                .and_then(|a| Ok(VAddress::new(a as usize)))
        } else if size <= MSize::new(1024) {
            self.slab_1024
                .alloc_ptr()
                .and_then(|a| Ok(VAddress::new(a as usize)))
        } else if size <= MSize::new(2048) {
            self.slab_2048
                .alloc_ptr()
                .and_then(|a| Ok(VAddress::new(a as usize)))
        } else if size <= MSize::new(4096) {
            self.slab_4096
                .alloc_ptr()
                .and_then(|a| Ok(VAddress::new(a as usize)))
        } else {
            Err(())
        }
    }

    pub fn add_pool(
        &mut self,
        slab_size_to_add: MSize,
        pool_address: VAddress,
        pool_size: MSize,
    ) -> Result<(), ()> {
        if slab_size_to_add <= MSize::new(64) {
            unsafe {
                self.slab_64
                    .add_pool(pool_address.to_usize(), pool_size.to_usize())
            }
        } else if slab_size_to_add <= MSize::new(128) {
            unsafe {
                self.slab_128
                    .add_pool(pool_address.to_usize(), pool_size.to_usize())
            }
        } else if slab_size_to_add <= MSize::new(256) {
            unsafe {
                self.slab_256
                    .add_pool(pool_address.to_usize(), pool_size.to_usize())
            }
        } else if slab_size_to_add <= MSize::new(512) {
            unsafe {
                self.slab_512
                    .add_pool(pool_address.to_usize(), pool_size.to_usize())
            }
        } else if slab_size_to_add <= MSize::new(1024) {
            unsafe {
                self.slab_1024
                    .add_pool(pool_address.to_usize(), pool_size.to_usize())
            }
        } else if slab_size_to_add <= MSize::new(2048) {
            unsafe {
                self.slab_2048
                    .add_pool(pool_address.to_usize(), pool_size.to_usize())
            }
        } else if slab_size_to_add <= MSize::new(4096) {
            unsafe {
                self.slab_4096
                    .add_pool(pool_address.to_usize(), pool_size.to_usize())
            }
        } else {
            return Err(());
        }
        return Ok(());
    }

    pub fn dealloc(&mut self, address: VAddress, size: MSize) {
        if size <= MSize::new(64) {
            self.slab_64.free_ptr(address.into());
        } else if size <= MSize::new(128) {
            self.slab_128.free_ptr(address.into());
        } else if size <= MSize::new(256) {
            self.slab_256.free_ptr(address.into());
        } else if size <= MSize::new(512) {
            self.slab_512.free_ptr(address.into());
        } else if size <= MSize::new(1024) {
            self.slab_1024.free_ptr(address.into());
        } else if size <= MSize::new(2048) {
            self.slab_2048.free_ptr(address.into());
        } else if size <= MSize::new(4096) {
            self.slab_4096.free_ptr(address.into());
        }
    }
}
