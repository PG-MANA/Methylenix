//!
//! Heap Allocator
//!

use crate::kernel::memory_manager::data_type::{Address, MPageOrder, MSize, VAddress};
use crate::kernel::memory_manager::{
    pool_allocator::PoolAllocator, MemoryError, MemoryManager, MemoryPermissionFlags,
};
use crate::kernel::sync::spin_lock::Mutex;

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
    const DEFAULT_ALLOC_PAGE_ORDER: MPageOrder = MPageOrder::new(2);

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
        let address = memory_manager.alloc_pages(
            Self::DEFAULT_ALLOC_PAGE_ORDER,
            MemoryPermissionFlags::data(),
        )?;
        unsafe {
            self.slab_64.set_initial_pool(
                address.to_usize(),
                Self::DEFAULT_ALLOC_PAGE_ORDER.to_offset().to_usize(),
            )
        };
        let address = memory_manager.alloc_pages(
            Self::DEFAULT_ALLOC_PAGE_ORDER,
            MemoryPermissionFlags::data(),
        )?;
        unsafe {
            self.slab_128.set_initial_pool(
                address.to_usize(),
                Self::DEFAULT_ALLOC_PAGE_ORDER.to_offset().to_usize(),
            )
        };
        let address = memory_manager.alloc_pages(
            Self::DEFAULT_ALLOC_PAGE_ORDER,
            MemoryPermissionFlags::data(),
        )?;
        unsafe {
            self.slab_256.set_initial_pool(
                address.to_usize(),
                Self::DEFAULT_ALLOC_PAGE_ORDER.to_offset().to_usize(),
            )
        };
        let address = memory_manager.alloc_pages(
            Self::DEFAULT_ALLOC_PAGE_ORDER,
            MemoryPermissionFlags::data(),
        )?;
        unsafe {
            self.slab_512.set_initial_pool(
                address.to_usize(),
                Self::DEFAULT_ALLOC_PAGE_ORDER.to_offset().to_usize(),
            )
        };
        let address = memory_manager.alloc_pages(
            Self::DEFAULT_ALLOC_PAGE_ORDER,
            MemoryPermissionFlags::data(),
        )?;
        unsafe {
            self.slab_1024.set_initial_pool(
                address.to_usize(),
                Self::DEFAULT_ALLOC_PAGE_ORDER.to_offset().to_usize(),
            )
        };
        let address = memory_manager.alloc_pages(
            Self::DEFAULT_ALLOC_PAGE_ORDER,
            MemoryPermissionFlags::data(),
        )?;
        unsafe {
            self.slab_2048.set_initial_pool(
                address.to_usize(),
                Self::DEFAULT_ALLOC_PAGE_ORDER.to_offset().to_usize(),
            )
        };
        let address = memory_manager.alloc_pages(
            Self::DEFAULT_ALLOC_PAGE_ORDER,
            MemoryPermissionFlags::data(),
        )?;
        unsafe {
            self.slab_4096.set_initial_pool(
                address.to_usize(),
                Self::DEFAULT_ALLOC_PAGE_ORDER.to_offset().to_usize(),
            )
        };
        Ok(())
    }

    pub fn alloc(
        &mut self,
        size: MSize,
        memory_manager: &Mutex<MemoryManager>,
    ) -> Result<VAddress, MemoryError> {
        if size <= MSize::new(64) {
            match self.slab_64.alloc_ptr() {
                Ok(a) => Ok(VAddress::new(a as usize)),
                Err(()) => {
                    let address = memory_manager.lock().unwrap().alloc_pages(
                        Self::DEFAULT_ALLOC_PAGE_ORDER,
                        MemoryPermissionFlags::data(),
                    )?;
                    for i in 1
                        ..(Self::DEFAULT_ALLOC_PAGE_ORDER.to_offset().to_usize() >> 6/* PAGE_SIZE / 64 */)
                    {
                        self.slab_64
                            .free_ptr((address.to_usize() + (i << 6)) as *mut _);
                    }
                    Ok(address)
                }
            }
        } else if size <= MSize::new(128) {
            match self.slab_128.alloc_ptr() {
                Ok(a) => Ok(VAddress::new(a as usize)),
                Err(()) => {
                    let address = memory_manager.lock().unwrap().alloc_pages(
                        Self::DEFAULT_ALLOC_PAGE_ORDER,
                        MemoryPermissionFlags::data(),
                    )?;
                    for i in 1
                        ..(Self::DEFAULT_ALLOC_PAGE_ORDER.to_offset().to_usize() >> 7/* PAGE_SIZE / 128 */)
                    {
                        self.slab_128
                            .free_ptr((address.to_usize() + (i << 7)) as *mut _);
                    }
                    Ok(address)
                }
            }
        } else if size <= MSize::new(256) {
            match self.slab_256.alloc_ptr() {
                Ok(a) => Ok(VAddress::new(a as usize)),
                Err(()) => {
                    let address = memory_manager.lock().unwrap().alloc_pages(
                        Self::DEFAULT_ALLOC_PAGE_ORDER,
                        MemoryPermissionFlags::data(),
                    )?;
                    for i in 1
                        ..(Self::DEFAULT_ALLOC_PAGE_ORDER.to_offset().to_usize() >> 8/* PAGE_SIZE / 256 */)
                    {
                        self.slab_256
                            .free_ptr((address.to_usize() + (i << 8)) as *mut _);
                    }
                    Ok(address)
                }
            }
        } else if size <= MSize::new(512) {
            match self.slab_512.alloc_ptr() {
                Ok(a) => Ok(VAddress::new(a as usize)),
                Err(()) => {
                    let address = memory_manager.lock().unwrap().alloc_pages(
                        Self::DEFAULT_ALLOC_PAGE_ORDER,
                        MemoryPermissionFlags::data(),
                    )?;
                    for i in 1
                        ..(Self::DEFAULT_ALLOC_PAGE_ORDER.to_offset().to_usize() >> 9/* PAGE_SIZE / 512 */)
                    {
                        self.slab_512
                            .free_ptr((address.to_usize() + (i << 9)) as *mut _);
                    }
                    Ok(address)
                }
            }
        } else if size <= MSize::new(1024) {
            match self.slab_1024.alloc_ptr() {
                Ok(a) => Ok(VAddress::new(a as usize)),
                Err(()) => {
                    let address = memory_manager.lock().unwrap().alloc_pages(
                        Self::DEFAULT_ALLOC_PAGE_ORDER,
                        MemoryPermissionFlags::data(),
                    )?;
                    for i in 1
                        ..(Self::DEFAULT_ALLOC_PAGE_ORDER.to_offset().to_usize() >> 10/* PAGE_SIZE / 512 */)
                    {
                        self.slab_1024
                            .free_ptr((address.to_usize() + (i << 10)) as *mut _);
                    }
                    Ok(address)
                }
            }
        } else if size <= MSize::new(2048) {
            match self.slab_2048.alloc_ptr() {
                Ok(a) => Ok(VAddress::new(a as usize)),
                Err(()) => {
                    let address = memory_manager.lock().unwrap().alloc_pages(
                        Self::DEFAULT_ALLOC_PAGE_ORDER,
                        MemoryPermissionFlags::data(),
                    )?;
                    for i in 1
                        ..(Self::DEFAULT_ALLOC_PAGE_ORDER.to_offset().to_usize() >> 11/* PAGE_SIZE / 2048 */)
                    {
                        self.slab_2048
                            .free_ptr((address.to_usize() + (i << 11)) as *mut _);
                    }
                    Ok(address)
                }
            }
        } else if size <= MSize::new(4096) {
            match self.slab_4096.alloc_ptr() {
                Ok(a) => Ok(VAddress::new(a as usize)),
                Err(()) => {
                    let address = memory_manager.lock().unwrap().alloc_pages(
                        Self::DEFAULT_ALLOC_PAGE_ORDER,
                        MemoryPermissionFlags::data(),
                    )?;
                    for i in 1
                        ..(Self::DEFAULT_ALLOC_PAGE_ORDER.to_offset().to_usize() >> 12/* PAGE_SIZE / 128 */)
                    {
                        self.slab_4096
                            .free_ptr((address.to_usize() + (i << 12)) as *mut _);
                    }
                    Ok(address)
                }
            }
        } else {
            Err(MemoryError::InvalidSize)
        }
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
