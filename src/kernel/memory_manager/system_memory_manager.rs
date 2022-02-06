//!
//! System Memory Manager
//!
//! This manager manages physical memory and struct entry's pools.
//! Only one SystemMemoryManager exists in the Kernel.

use super::data_type::{
    Address, MOrder, MPageOrder, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress,
};
use super::physical_memory_manager::PhysicalMemoryManager;
use super::slab_allocator::pool_allocator::PoolAllocator;
use super::virtual_memory_manager::{
    VirtualMemoryEntry, VirtualMemoryManager, VirtualMemoryObject, VirtualMemoryPage,
};
use super::{MemoryError, MemoryManager};

use crate::arch::target_arch::context::memory_layout::physical_address_to_direct_map;
use crate::arch::target_arch::paging::{PAGE_SHIFT, PAGE_SIZE};

use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};

use crate::kernel::sync::spin_lock::IrqSaveSpinLockFlag;
use crate::kernel::task_manager::work_queue::WorkList;

use crate::alloc_pages;

pub struct SystemMemoryManager {
    lock: IrqSaveSpinLockFlag,
    original_physical_memory_manager: PhysicalMemoryManager,
    vm_entry_pool: PoolAllocator<VirtualMemoryEntry>,
    vm_object_pool: PoolAllocator<VirtualMemoryObject>,
    vm_page_pool: PoolAllocator<VirtualMemoryPage>,
}

impl SystemMemoryManager {
    const VM_ENTRY_LOW: usize = 16;
    const VM_ENTRY_RESERVE: usize = 8;
    const VM_OBJECT_LOW: usize = 16;
    const VM_OBJECT_RESERVE: usize = 8;
    const VM_PAGE_LOW: usize = 16;
    const VM_PAGE_RESERVE: usize = 8;

    const PAGE_ORDER_VM_ENTRY_POOL: MPageOrder = MPageOrder::new(0);
    const PAGE_ORDER_VM_OBJECT_POOL: MPageOrder = MPageOrder::new(0);
    const PAGE_ORDER_VM_PAGE_POOL: MPageOrder = MPageOrder::new(2);

    pub(super) const ALLOC_VM_ENTRY_FLAG: usize = 1 << 0;
    pub(super) const ALLOC_VM_OBJECT_FLAG: usize = 1 << 1;
    pub(super) const ALLOC_VM_PAGE_FLAG: usize = 1 << 2;

    pub const fn new(physical_memory_manager: PhysicalMemoryManager) -> Self {
        Self {
            lock: IrqSaveSpinLockFlag::new(),
            original_physical_memory_manager: physical_memory_manager,
            vm_entry_pool: PoolAllocator::new(),
            vm_object_pool: PoolAllocator::new(),
            vm_page_pool: PoolAllocator::new(),
        }
    }
    pub fn init_pools(&mut self, _vm_manager: &mut VirtualMemoryManager) {
        const VM_MAP_ENTRY_POOL_SIZE: MSize = PAGE_SIZE << MSize::new(3);
        const VM_OBJECT_POOL_SIZE: MSize = PAGE_SIZE << MSize::new(2);
        const VM_PAGE_POOL_SIZE: MSize = PAGE_SIZE << MSize::new(7);

        let alloc_func = |size: MSize, p: &mut PhysicalMemoryManager| -> usize {
            p.alloc(size, MOrder::new(PAGE_SHIFT))
                .and_then(|p| Ok(physical_address_to_direct_map(p).to_usize()))
                .expect("Failed to allocate memory")
        };
        let _lock = self.lock.lock();
        let pm_manager = &mut self.original_physical_memory_manager;

        unsafe {
            self.vm_page_pool.add_pool(
                alloc_func(VM_PAGE_POOL_SIZE, pm_manager),
                VM_PAGE_POOL_SIZE.to_usize(),
            );
            self.vm_object_pool.add_pool(
                alloc_func(VM_OBJECT_POOL_SIZE, pm_manager),
                VM_OBJECT_POOL_SIZE.to_usize(),
            );
            self.vm_entry_pool.add_pool(
                alloc_func(VM_MAP_ENTRY_POOL_SIZE, pm_manager),
                VM_MAP_ENTRY_POOL_SIZE.to_usize(),
            );
        }
        drop(_lock);
    }

    /// [`Self::lock`] must be unlocked.
    fn add_vm_entry_pool(&mut self) -> Result<(), MemoryError> {
        match alloc_pages!(
            Self::PAGE_ORDER_VM_ENTRY_POOL,
            MemoryPermissionFlags::data(),
            MemoryOptionFlags::KERNEL | MemoryOptionFlags::WIRED | MemoryOptionFlags::CRITICAL
        ) {
            Ok(address) => {
                let _lock = self.lock.lock();
                unsafe {
                    self.vm_entry_pool.add_pool(
                        address.to_usize(),
                        Self::PAGE_ORDER_VM_ENTRY_POOL.to_offset().to_usize(),
                    )
                };
                Ok(())
            }
            Err(MemoryError::EntryPoolRunOut) => {
                /* TODO: Remake */
                let pm_manager = &mut self.original_physical_memory_manager;
                if let Err(e) = MemoryManager::add_physical_memory_manager_pool(pm_manager) {
                    pr_err!("Failed to add PhysicalMemoryManager's memory pool: {:?}", e);
                    Err(e)
                } else {
                    self.add_vm_entry_pool()
                }
            }
            Err(e) => {
                pr_err!("Failed to allocate vm_entry: {:?}", e);

                Err(e)
            }
        }
    }

    /// [`Self::lock`] must be unlocked.
    fn add_vm_object_pool(&mut self) -> Result<(), MemoryError> {
        match alloc_pages!(
            Self::PAGE_ORDER_VM_OBJECT_POOL,
            MemoryPermissionFlags::data(),
            MemoryOptionFlags::KERNEL | MemoryOptionFlags::WIRED | MemoryOptionFlags::CRITICAL
        ) {
            Ok(address) => {
                let _lock = self.lock.lock();
                unsafe {
                    self.vm_object_pool.add_pool(
                        address.to_usize(),
                        Self::PAGE_ORDER_VM_OBJECT_POOL.to_offset().to_usize(),
                    )
                };
                Ok(())
            }
            Err(MemoryError::EntryPoolRunOut) => {
                /* TODO: Remake */
                let pm_manager = &mut self.original_physical_memory_manager;
                if let Err(e) = MemoryManager::add_physical_memory_manager_pool(pm_manager) {
                    pr_err!("Failed to add PhysicalMemoryManager's memory pool: {:?}", e);
                    Err(e)
                } else {
                    self.add_vm_object_pool()
                }
            }
            Err(e) => {
                pr_err!("Failed to allocate vm_object: {:?}", e);
                Err(e)
            }
        }
    }

    /// [`Self::lock`] must be unlocked.
    fn add_vm_page_pool(&mut self) -> Result<(), MemoryError> {
        match alloc_pages!(
            Self::PAGE_ORDER_VM_PAGE_POOL,
            MemoryPermissionFlags::data(),
            MemoryOptionFlags::KERNEL | MemoryOptionFlags::WIRED | MemoryOptionFlags::CRITICAL
        ) {
            Ok(address) => {
                let _lock = self.lock.lock();
                unsafe {
                    self.vm_page_pool.add_pool(
                        address.to_usize(),
                        Self::PAGE_ORDER_VM_PAGE_POOL.to_offset().to_usize(),
                    )
                };
                Ok(())
            }
            Err(MemoryError::EntryPoolRunOut) => {
                /* TODO: Remake */
                let pm_manager = &mut self.original_physical_memory_manager;
                if let Err(e) = MemoryManager::add_physical_memory_manager_pool(pm_manager) {
                    pr_err!("Failed to add PhysicalMemoryManager's memory pool: {:?}", e);
                    Err(e)
                } else {
                    self.add_vm_page_pool()
                }
            }
            Err(e) => {
                pr_err!("Failed to allocate vm_entry: {:?}", e);

                Err(e)
            }
        }
    }

    pub fn alloc_vm_entry(
        &mut self,
        is_system_memory_manager: bool,
        option: MemoryOptionFlags,
    ) -> Result<&'static mut VirtualMemoryEntry, MemoryError> {
        let _lock = self.lock.lock();
        let count = self.vm_entry_pool.get_count();
        if count > Self::VM_ENTRY_RESERVE {
            let result = self.vm_entry_pool.alloc();
            if let Ok(e) = result {
                if count - 1 <= Self::VM_ENTRY_LOW {
                    if let Err(_) = get_cpu_manager_cluster().work_queue.add_work(WorkList::new(
                        Self::pool_alloc_worker,
                        Self::ALLOC_VM_ENTRY_FLAG,
                    )) {
                        pr_err!("Failed to add worker for memory allocator.");
                    }
                }
                return Ok(e);
            }
            pr_err!("Failed to allocate vm_entry");
        } else if option.is_critical() {
            let entry = self
                .vm_entry_pool
                .alloc()
                .or(Err(MemoryError::EntryPoolRunOut))?;
            drop(_lock);
            return Ok(entry);
        } else if is_system_memory_manager {
            return Err(MemoryError::EntryPoolRunOut);
        }
        drop(_lock);
        self.add_vm_entry_pool()?;
        return self.alloc_vm_entry(is_system_memory_manager, option);
    }

    pub fn alloc_vm_object(
        &mut self,
        is_system_memory_manager: bool,
        option: MemoryOptionFlags,
    ) -> Result<&'static mut VirtualMemoryObject, MemoryError> {
        let _lock = self.lock.lock();
        let count = self.vm_object_pool.get_count();
        if count > Self::VM_OBJECT_RESERVE {
            let result = self.vm_object_pool.alloc();
            if let Ok(e) = result {
                if count - 1 <= Self::VM_OBJECT_LOW {
                    if let Err(_) = get_cpu_manager_cluster().work_queue.add_work(WorkList::new(
                        Self::pool_alloc_worker,
                        Self::ALLOC_VM_OBJECT_FLAG,
                    )) {
                        pr_err!("Failed to add worker for memory allocator.");
                    }
                }
                return Ok(e);
            }
            pr_err!("Failed to allocate vm_object");
        } else if option.is_critical() {
            let entry = self
                .vm_object_pool
                .alloc()
                .or(Err(MemoryError::EntryPoolRunOut))?;
            drop(_lock);
            return Ok(entry);
        } else if is_system_memory_manager {
            return Err(MemoryError::EntryPoolRunOut);
        }
        drop(_lock);
        self.add_vm_object_pool()?;
        return self.alloc_vm_object(is_system_memory_manager, option);
    }

    pub fn alloc_vm_page(
        &mut self,
        _physical_address: PAddress,
        is_system_memory_manager: bool,
        option: MemoryOptionFlags,
    ) -> Result<&'static mut VirtualMemoryPage, MemoryError> {
        let _lock = self.lock.lock();
        let count = self.vm_page_pool.get_count();
        if count > Self::VM_PAGE_RESERVE {
            let result = self.vm_page_pool.alloc();
            if let Ok(e) = result {
                if count - 1 <= Self::VM_PAGE_LOW {
                    if let Err(_) = get_cpu_manager_cluster().work_queue.add_work(WorkList::new(
                        Self::pool_alloc_worker,
                        Self::ALLOC_VM_PAGE_FLAG,
                    )) {
                        pr_err!("Failed to add worker for memory allocator.");
                    }
                }
                return Ok(e);
            }
            pr_err!("Failed to allocate vm_page");
        } else if option.is_critical() {
            let entry = self
                .vm_page_pool
                .alloc()
                .or(Err(MemoryError::EntryPoolRunOut))?;
            drop(_lock);
            return Ok(entry);
        } else if is_system_memory_manager {
            return Err(MemoryError::EntryPoolRunOut);
        }
        drop(_lock);
        self.add_vm_page_pool()?;
        return self.alloc_vm_page(_physical_address, is_system_memory_manager, option);
    }

    pub fn free_vm_entry(&mut self, vm_entry: &'static mut VirtualMemoryEntry) {
        let _lock = self.lock.lock();
        self.vm_entry_pool.free(vm_entry)
    }

    pub fn free_vm_object(&mut self, vm_object: &'static mut VirtualMemoryObject) {
        let _lock = self.lock.lock();
        self.vm_object_pool.free(vm_object)
    }

    pub fn free_vm_page(
        &mut self,
        vm_page: &'static mut VirtualMemoryPage,
        _physical_address: PAddress,
    ) {
        let _lock = self.lock.lock();
        self.vm_page_pool.free(vm_page)
    }

    pub(super) fn pool_alloc_worker(flag: usize) {
        let s = &mut get_kernel_manager_cluster().system_memory_manager;

        if s.vm_page_pool.get_count() <= Self::VM_PAGE_LOW || (flag & Self::ALLOC_VM_PAGE_FLAG) != 0
        {
            if let Err(e) = s.add_vm_page_pool() {
                pr_err!("Failed to alloc memory for vm_entry: {:?}", e);
            }
        }

        if s.vm_object_pool.get_count() <= Self::VM_OBJECT_LOW
            || (flag & Self::ALLOC_VM_OBJECT_FLAG) != 0
        {
            if let Err(e) = s.add_vm_object_pool() {
                pr_err!("Failed to alloc memory for vm_object: {:?}", e);
            }
        }

        if s.vm_entry_pool.get_count() <= Self::VM_ENTRY_LOW
            || (flag & Self::ALLOC_VM_ENTRY_FLAG) != 0
        {
            if let Err(e) = s.add_vm_entry_pool() {
                pr_err!("Failed to alloc memory for vm_entry: {:?}", e);
            }
        }
    }
}

pub fn get_physical_memory_manager() -> &'static mut PhysicalMemoryManager {
    &mut get_kernel_manager_cluster()
        .system_memory_manager
        .original_physical_memory_manager
}
