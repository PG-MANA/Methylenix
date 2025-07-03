//!
//! Memory Allocator
//!
//! This is the front end of the memory management system.
//! The Object allocator is used when the system needs to allocate a small object which will be freed soon.
//!

use super::MemoryError;
use super::data_type::{MSize, MemoryPermissionFlags, VAddress};
use super::slab_allocator::LocalSlabAllocator;

use crate::arch::target_arch::paging::{PAGE_MASK, PAGE_SIZE};

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{Address, MemoryOptionFlags};

use core::ptr::NonNull;

struct SizeAllocator {
    size_64: LocalSlabAllocator<[u8; 64]>,
    size_128: LocalSlabAllocator<[u8; 128]>,
    size_256: LocalSlabAllocator<[u8; 256]>,
    size_512: LocalSlabAllocator<[u8; 512]>,
    size_1024: LocalSlabAllocator<[u8; 1024]>,
    size_2048: LocalSlabAllocator<[u8; 2048]>,
    size_4096: LocalSlabAllocator<[u8; 4096]>,
}

pub struct MemoryAllocator {
    size_allocator: SizeAllocator,
}

impl SizeAllocator {
    const MAX_SIZE: MSize = MSize::new(4096);

    const fn new() -> Self {
        Self {
            size_64: LocalSlabAllocator::new(),
            size_128: LocalSlabAllocator::new(),
            size_256: LocalSlabAllocator::new(),
            size_512: LocalSlabAllocator::new(),
            size_1024: LocalSlabAllocator::new(),
            size_2048: LocalSlabAllocator::new(),
            size_4096: LocalSlabAllocator::new(),
        }
    }

    fn init(&mut self) -> Result<(), MemoryError> {
        self.size_64.init()?;
        self.size_128.init()?;
        self.size_256.init()?;
        self.size_512.init()?;
        self.size_1024.init()?;
        self.size_2048.init()?;
        self.size_4096.init()?;
        Ok(())
    }

    pub fn alloc(&mut self, size: MSize) -> Result<VAddress, MemoryError> {
        if size <= MSize::new(64) {
            self.size_64
                .alloc()
                .map(|a| VAddress::from(a.as_ptr() as usize))
        } else if size <= MSize::new(128) {
            self.size_128
                .alloc()
                .map(|a| VAddress::from(a.as_ptr() as usize))
        } else if size <= MSize::new(256) {
            self.size_256
                .alloc()
                .map(|a| VAddress::from(a.as_ptr() as usize))
        } else if size <= MSize::new(512) {
            self.size_512
                .alloc()
                .map(|a| VAddress::from(a.as_ptr() as usize))
        } else if size <= MSize::new(1024) {
            self.size_1024
                .alloc()
                .map(|a| VAddress::from(a.as_ptr() as usize))
        } else if size <= MSize::new(2048) {
            self.size_2048
                .alloc()
                .map(|a| VAddress::from(a.as_ptr() as usize))
        } else if size <= MSize::new(4096) {
            self.size_4096
                .alloc()
                .map(|a| VAddress::from(a.as_ptr() as usize))
        } else {
            Err(MemoryError::InvalidSize)
        }
    }

    pub fn dealloc(&mut self, address: VAddress, size: MSize) {
        if size <= MSize::new(64) {
            self.size_64
                .free(NonNull::new(address.to_usize() as *mut _).unwrap());
        } else if size <= MSize::new(128) {
            self.size_128
                .free(NonNull::new(address.to_usize() as *mut _).unwrap());
        } else if size <= MSize::new(256) {
            self.size_256
                .free(NonNull::new(address.to_usize() as *mut _).unwrap());
        } else if size <= MSize::new(512) {
            self.size_512
                .free(NonNull::new(address.to_usize() as *mut _).unwrap());
        } else if size <= MSize::new(1024) {
            self.size_1024
                .free(NonNull::new(address.to_usize() as *mut _).unwrap());
        } else if size <= MSize::new(2048) {
            self.size_2048
                .free(NonNull::new(address.to_usize() as *mut _).unwrap());
        } else if size <= MSize::new(4096) {
            self.size_4096
                .free(NonNull::new(address.to_usize() as *mut _).unwrap());
        }
    }
}

impl MemoryAllocator {
    pub const fn new() -> Self {
        Self {
            size_allocator: SizeAllocator::new(),
        }
    }

    pub fn init(&mut self) -> Result<(), MemoryError> {
        self.size_allocator.init()
    }

    pub fn kmalloc(&mut self, size: MSize) -> Result<VAddress, MemoryError> {
        if size.is_zero() {
            Err(MemoryError::InvalidSize)
        } else if size > SizeAllocator::MAX_SIZE {
            let page_aligned_size = MSize::new((size - MSize::new(1)) & PAGE_MASK) + PAGE_SIZE;
            get_kernel_manager_cluster()
                .kernel_memory_manager
                .alloc_pages(
                    page_aligned_size.to_order(None).to_page_order(),
                    MemoryPermissionFlags::data(),
                    Some(MemoryOptionFlags::KERNEL | MemoryOptionFlags::ALLOC),
                )
        } else {
            self.size_allocator.alloc(size)
        }
    }

    pub fn kfree(&mut self, address: VAddress, size: MSize) -> Result<(), MemoryError> {
        if size.is_zero() {
            Err(MemoryError::InvalidSize)
        } else if size > SizeAllocator::MAX_SIZE {
            get_kernel_manager_cluster()
                .kernel_memory_manager
                .free(address)
        } else {
            self.size_allocator.dealloc(address, size);
            Ok(())
        }
    }

    /// [`Self::kfree`] with [`core::ptr::drop_in_place`].
    /// Regardless of the result, `data` will be dropped
    pub fn kfree_data<T: Sized>(&mut self, data: &mut T) -> Result<(), MemoryError> {
        let size = MSize::new(size_of_val(data));
        let address = VAddress::new(data as *mut _ as usize);
        unsafe { core::ptr::drop_in_place(data) };
        if size.is_zero() {
            Ok(())
        } else {
            self.kfree(address, size)
        }
    }

    pub fn vmalloc(&mut self, size: MSize) -> Result<VAddress, MemoryError> {
        if size.is_zero() {
            return Err(MemoryError::InvalidSize);
        }
        let page_aligned_size = MSize::new((size - MSize::new(1)) & PAGE_MASK) + PAGE_SIZE;
        get_kernel_manager_cluster()
            .kernel_memory_manager
            .alloc_nonlinear_pages(
                page_aligned_size,
                MemoryPermissionFlags::data(),
                Some(MemoryOptionFlags::KERNEL | MemoryOptionFlags::ALLOC),
            )
    }

    pub fn vfree(&mut self, address: VAddress) -> Result<(), MemoryError> {
        get_kernel_manager_cluster()
            .kernel_memory_manager
            .free(address)
    }
}
