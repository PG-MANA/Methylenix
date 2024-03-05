//!
//! Slab Allocator
//!
//! This allocator is used to allocate specific object.
//!

pub mod pool_allocator;

use self::pool_allocator::PoolAllocator;

use super::data_type::{Address, MPageOrder, MemoryPermissionFlags};
use super::{alloc_pages, MemoryError};

use crate::arch::target_arch::interrupt::InterruptManager;

use crate::kernel::sync::spin_lock::IrqSaveSpinLockFlag;

struct SlabAllocator<T> {
    allocator: PoolAllocator<T>,
}

pub struct LocalSlabAllocator<T> {
    slab_allocator: SlabAllocator<T>,
}

pub struct GlobalSlabAllocator<T> {
    lock: IrqSaveSpinLockFlag,
    slab_allocator: SlabAllocator<T>,
}

impl<T> SlabAllocator<T> {
    const DEFAULT_ALLOC_ORDER: MPageOrder = MPageOrder::new(2);

    pub const fn new(align: usize) -> Self {
        Self {
            allocator: PoolAllocator::new_with_align(align),
        }
    }

    pub fn init(&mut self) -> Result<(), MemoryError> {
        self.grow_pool()
    }

    fn grow_pool(&mut self) -> Result<(), MemoryError> {
        let page = alloc_pages!(Self::DEFAULT_ALLOC_ORDER, MemoryPermissionFlags::data())?;
        unsafe {
            self.allocator.add_pool(
                page.to_usize(),
                Self::DEFAULT_ALLOC_ORDER.to_offset().to_usize(),
            )
        };
        Ok(())
    }

    pub fn alloc(&mut self) -> Result<&'static mut T, MemoryError> {
        match self.allocator.alloc() {
            Ok(e) => Ok(e),
            Err(_) => {
                self.grow_pool()?;
                self.alloc()
            }
        }
    }

    pub fn free(&mut self, entry: &'static mut T) {
        self.allocator.free(entry);
    }

    pub fn len(&self) -> usize {
        self.allocator.get_count()
    }
}

impl<T> LocalSlabAllocator<T> {
    pub const fn new(align: usize) -> Self {
        Self {
            slab_allocator: SlabAllocator::new(align),
        }
    }

    pub fn init(&mut self) -> Result<(), MemoryError> {
        let irq = InterruptManager::save_and_disable_local_irq();
        let result = self.slab_allocator.init();
        InterruptManager::restore_local_irq(irq);
        result
    }

    pub fn alloc(&mut self) -> Result<&'static mut T, MemoryError> {
        let irq = InterruptManager::save_and_disable_local_irq();
        let result = self.slab_allocator.alloc();
        InterruptManager::restore_local_irq(irq);
        result
    }

    pub fn free(&mut self, entry: &'static mut T) {
        let irq = InterruptManager::save_and_disable_local_irq();
        self.slab_allocator.free(entry);
        InterruptManager::restore_local_irq(irq);
    }

    pub fn len(&self) -> usize {
        let irq = InterruptManager::save_and_disable_local_irq();
        let result = self.slab_allocator.len();
        InterruptManager::restore_local_irq(irq);
        result
    }
}

impl<T> GlobalSlabAllocator<T> {
    pub const fn new(align: usize) -> Self {
        Self {
            lock: IrqSaveSpinLockFlag::new(),
            slab_allocator: SlabAllocator::new(align),
        }
    }

    pub fn init(&mut self) -> Result<(), MemoryError> {
        let _lock = self.lock.lock();
        let result = self.slab_allocator.init();
        drop(_lock);
        result
    }

    pub fn alloc(&mut self) -> Result<&'static mut T, MemoryError> {
        let _lock = self.lock.lock();
        let result = self.slab_allocator.alloc();
        drop(_lock);
        result
    }

    pub fn free(&mut self, entry: &'static mut T) {
        let _lock = self.lock.lock();
        self.slab_allocator.free(entry);
        drop(_lock);
    }

    pub fn len(&self) -> usize {
        let _lock = self.lock.lock();
        let result = self.slab_allocator.len();
        drop(_lock);
        result
    }
}
