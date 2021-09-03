//!
//! Object Allocator
//!
//! This is the front end of memory management system.
//! The Object allocator is used when the system needs to allocate small object which will be freed soon.
//!

pub mod cache_allocator;
mod heap_allocator;

use self::heap_allocator::HeapAllocator;

use crate::arch::target_arch::interrupt::InterruptManager;
use crate::arch::target_arch::paging::PAGE_SIZE;

use crate::kernel::memory_manager::data_type::{MPageOrder, MSize, VAddress};
use crate::kernel::memory_manager::{MemoryError, MemoryManager, MemoryPermissionFlags};
use crate::kernel::sync::spin_lock::Mutex;

pub struct ObjectAllocator {
    heap_allocator: HeapAllocator,
    page_cache: [VAddress; Self::PAGE_CACHE_LEN],
}

impl ObjectAllocator {
    const PAGE_CACHE_LEN: usize = 8;

    pub const fn new() -> Self {
        Self {
            heap_allocator: HeapAllocator::new(),
            page_cache: [VAddress::new(0); Self::PAGE_CACHE_LEN],
        }
    }

    pub fn init(&mut self, m_manager: &mut MemoryManager) -> bool {
        let irq = InterruptManager::save_and_disable_local_irq();
        if let Err(e) = self.heap_allocator.init(m_manager) {
            InterruptManager::restore_local_irq(irq);
            pr_err!("Setting up ObjectAllocator is failed: {:?}", e);
            return false;
        }
        for e in &mut self.page_cache {
            *e = match m_manager.alloc_pages(MPageOrder::new(0), MemoryPermissionFlags::data()) {
                Ok(a) => a,
                Err(e) => {
                    InterruptManager::restore_local_irq(irq);
                    pr_err!("Failed to allocate page cache: {:?}", e);
                    return false;
                }
            };
        }
        InterruptManager::restore_local_irq(irq);
        return true;
    }

    fn alloc_page_from_cache(&mut self) -> Result<VAddress, ()> {
        const INVALID_ADDRESS: VAddress = VAddress::new(0);
        for e in self.page_cache.iter_mut().rev() {
            if *e != INVALID_ADDRESS {
                let a = *e;
                *e = INVALID_ADDRESS;
                return Ok(a);
            }
        }
        return Err(());
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
            let irq = InterruptManager::save_and_disable_local_irq();
            match self.heap_allocator.alloc(size) {
                Ok(a) => {
                    InterruptManager::restore_local_irq(irq);
                    Ok(a)
                }
                Err(_) => {
                    let pool = match self.alloc_page_from_cache() {
                        Ok(a) => a,
                        Err(_) => memory_manager
                            .lock()
                            .unwrap()
                            .alloc_pages(MPageOrder::new(0), MemoryPermissionFlags::data())?,
                    };
                    self.heap_allocator
                        .add_pool(size, pool, PAGE_SIZE)
                        .or(Err(MemoryError::InvalidSize))?;
                    let result = self
                        .heap_allocator
                        .alloc(size)
                        .or(Err(MemoryError::AddressNotAvailable));
                    InterruptManager::restore_local_irq(irq);
                    result
                }
            }
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
            let irq = InterruptManager::save_and_disable_local_irq();
            self.heap_allocator.dealloc(address, size);
            InterruptManager::restore_local_irq(irq);
            Ok(())
        }
    }
}
