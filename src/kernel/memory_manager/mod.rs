//!
//! Memory Manager
//!
//! This manager is the frontend of physical memory manager and page manager.
//! In this memory system, you should not use alloc::*, use only core::*
//!

pub mod data_type;
pub mod global_allocator;
pub mod memory_allocator;
pub mod physical_memory_manager;
pub mod slab_allocator;
pub mod virtual_memory_manager;

use self::data_type::{
    Address, MIndex, MOrder, MPageOrder, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress,
    VAddress,
};
use self::physical_memory_manager::PhysicalMemoryManager;
use self::virtual_memory_manager::{VirtualMemoryEntry, VirtualMemoryManager, VirtualMemoryPage};

use crate::arch::target_arch::context::memory_layout::physical_address_to_direct_map;
use crate::arch::target_arch::paging::{
    NEED_COPY_HIGH_MEMORY_PAGE_TABLE, PAGE_MASK, PAGE_SHIFT, PAGE_SIZE,
};

use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::slab_allocator::pool_allocator::PoolAllocator;
use crate::kernel::sync::spin_lock::IrqSaveSpinLockFlag;
use crate::kernel::task_manager::work_queue::WorkList;

pub struct MemoryManager {
    lock: IrqSaveSpinLockFlag,
    virtual_memory_manager: VirtualMemoryManager,
}

/* To share PhysicalMemoryManager */
pub struct SystemMemoryManager {
    lock: IrqSaveSpinLockFlag,
    original_physical_memory_manager: PhysicalMemoryManager,
    vm_entry_pool: PoolAllocator<VirtualMemoryEntry>,
    /*vm_object_pool: PoolAllocator<VirtualMemoryObject>,*/
    vm_page_pool: PoolAllocator<VirtualMemoryPage>,
}

#[derive(Clone, Eq, PartialEq, Copy, Debug)]
pub enum MemoryError {
    NotAligned,
    InvalidSize,
    InvalidAddress,
    AllocAddressFailed,
    FreeAddressFailed,
    AddressNotAvailable,
    MapAddressFailed,
    InternalError,
    EntryPoolRunOut,
    PagingError,
}

impl SystemMemoryManager {
    const VM_ENTRY_LOW: usize = 16;
    const VM_ENTRY_RESERVE: usize = 8;
    const VM_PAGE_LOW: usize = 16;
    const VM_PAGE_RESERVE: usize = 8;

    pub const fn new(physical_memory_manager: PhysicalMemoryManager) -> Self {
        Self {
            lock: IrqSaveSpinLockFlag::new(),
            original_physical_memory_manager: physical_memory_manager,
            vm_entry_pool: PoolAllocator::new(),
            vm_page_pool: PoolAllocator::new(),
        }
    }

    pub fn create_new_memory_manager(
        &'static mut self,
        virtual_memory_manager: VirtualMemoryManager,
    ) -> MemoryManager {
        MemoryManager::new(virtual_memory_manager)
    }

    pub fn init_pools(&mut self, _vm_manager: &mut VirtualMemoryManager) {
        const VM_MAP_ENTRY_POOL_SIZE: MSize = PAGE_SIZE << MSize::new(3);
        /*const VM_OBJECT_POOL_SIZE: usize = PAGE_SIZE * 8;*/
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
            self.vm_entry_pool.add_pool(
                alloc_func(VM_MAP_ENTRY_POOL_SIZE, pm_manager),
                VM_MAP_ENTRY_POOL_SIZE.to_usize(),
            );
        }
        drop(_lock);
    }

    /// [`Self::lock`] must be unlocked.
    fn add_vm_entry_pool(&mut self) -> Result<(), MemoryError> {
        let alloc_page_order = MPageOrder::new(0);
        match get_kernel_manager_cluster()
            .memory_manager
            .alloc_pages_with_option(
                alloc_page_order,
                MemoryPermissionFlags::data(),
                MemoryOptionFlags::KERNEL | MemoryOptionFlags::WIRED,
            ) {
            Ok(address) => {
                let _lock = self.lock.lock();
                unsafe {
                    self.vm_entry_pool
                        .add_pool(address.to_usize(), alloc_page_order.to_offset().to_usize())
                };
                Ok(())
            }
            Err(MemoryError::EntryPoolRunOut) => {
                let pm_manager = &mut self.original_physical_memory_manager;
                if let Err(e) = get_kernel_manager_cluster()
                    .memory_manager
                    .virtual_memory_manager
                    .add_physical_memory_manager_pool(pm_manager)
                {
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

    pub fn alloc_vm_entry(
        &mut self,
        is_for_system_entry: bool,
    ) -> Result<&'static mut VirtualMemoryEntry, MemoryError> {
        let _lock = self.lock.lock();
        let count = self.vm_entry_pool.get_count();
        if count > Self::VM_ENTRY_RESERVE {
            let result = self.vm_entry_pool.alloc();
            if let Ok(e) = result {
                if count - 1 <= Self::VM_ENTRY_LOW {
                    if let Err(_) = get_cpu_manager_cluster()
                        .work_queue
                        .add_work(WorkList::new(Self::pool_alloc_worker, 0))
                    {
                        pr_err!("Failed to add worker for memory allocator.");
                    }
                }
                return Ok(e);
            }
            pr_err!("Failed to allocate vm_entry");
        } else if is_for_system_entry {
            let entry = self
                .vm_entry_pool
                .alloc()
                .or(Err(MemoryError::EntryPoolRunOut))?;

            if let Err(_) = get_cpu_manager_cluster()
                .work_queue
                .add_work(WorkList::new(Self::pool_alloc_worker, 0))
            {
                pr_err!("Failed to add worker for memory allocator.");
            }
            return Ok(entry);
        }
        drop(_lock);
        self.add_vm_entry_pool()?;
        return self.alloc_vm_entry(is_for_system_entry);
    }

    pub fn free_vm_entry(&mut self, vm_entry: &'static mut VirtualMemoryEntry) {
        let _lock = self.lock.lock();
        self.vm_entry_pool.free(vm_entry)
    }

    /// [`Self::lock`] must be unlocked.
    fn add_vm_page_pool(&mut self) -> Result<(), MemoryError> {
        let alloc_page_order = MPageOrder::new(0);
        match get_kernel_manager_cluster()
            .memory_manager
            .alloc_pages_with_option(
                alloc_page_order,
                MemoryPermissionFlags::data(),
                MemoryOptionFlags::KERNEL | MemoryOptionFlags::WIRED,
            ) {
            Ok(address) => {
                let _lock = self.lock.lock();
                unsafe {
                    self.vm_page_pool
                        .add_pool(address.to_usize(), alloc_page_order.to_offset().to_usize())
                };
                Ok(())
            }
            Err(MemoryError::EntryPoolRunOut) => {
                let mut pm_manager = &mut self.original_physical_memory_manager;
                if let Err(e) = get_kernel_manager_cluster()
                    .memory_manager
                    .virtual_memory_manager
                    .add_physical_memory_manager_pool(&mut pm_manager)
                {
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

    pub fn alloc_vm_page(
        &mut self,
        is_for_system_entry: bool,
    ) -> Result<&'static mut VirtualMemoryPage, MemoryError> {
        let _lock = self.lock.lock();
        let count = self.vm_page_pool.get_count();
        if count > Self::VM_PAGE_RESERVE {
            let result = self.vm_page_pool.alloc();
            if let Ok(e) = result {
                if count - 1 <= Self::VM_PAGE_LOW {
                    if let Err(_) = get_cpu_manager_cluster()
                        .work_queue
                        .add_work(WorkList::new(Self::pool_alloc_worker, 0))
                    {
                        pr_err!("Failed to add worker for memory allocator.");
                    }
                }
                return Ok(e);
            }
            pr_err!("Failed to allocate vm_page");
        } else if is_for_system_entry {
            let entry = self
                .vm_page_pool
                .alloc()
                .or(Err(MemoryError::EntryPoolRunOut))?;
            if let Err(_) = get_cpu_manager_cluster()
                .work_queue
                .add_work(WorkList::new(Self::pool_alloc_worker, 0))
            {
                pr_err!("Failed to add worker for memory allocator.");
            }
            return Ok(entry);
        }
        drop(_lock);
        self.add_vm_page_pool()?;
        return self.alloc_vm_page(is_for_system_entry);
    }

    pub fn free_vm_page(&mut self, vm_page: &'static mut VirtualMemoryPage) {
        let _lock = self.lock.lock();
        self.vm_page_pool.free(vm_page)
    }

    fn pool_alloc_worker(_: usize) {
        let s = &mut get_kernel_manager_cluster().system_memory_manager;

        if s.vm_page_pool.get_count() <= Self::VM_PAGE_LOW {
            if let Err(e) = s.add_vm_page_pool() {
                pr_err!("Failed to alloc memory for vm_entry: {:?}", e);
            }
        }

        if s.vm_entry_pool.get_count() <= Self::VM_ENTRY_LOW {
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

impl MemoryManager {
    pub fn new(virtual_memory_manager: VirtualMemoryManager) -> Self {
        Self {
            virtual_memory_manager,
            lock: IrqSaveSpinLockFlag::new(),
        }
    }

    pub fn disable(&mut self) {
        let _lock = self.lock.lock();
        self.virtual_memory_manager.disable();
    }

    pub fn clone_kernel_memory_if_needed(&mut self) -> Result<(), MemoryError> {
        /* Depend on the architecture */
        if !NEED_COPY_HIGH_MEMORY_PAGE_TABLE {
            return Ok(());
        }
        if self
            .virtual_memory_manager
            .is_kernel_virtual_memory_manager()
        {
            return Ok(());
        }
        let _lock = self.lock.lock();
        let kernel_memory_manager = &get_kernel_manager_cluster().memory_manager;
        let _system_lock = kernel_memory_manager.lock.lock();
        let result = self
            .virtual_memory_manager
            .clone_kernel_area(&kernel_memory_manager.virtual_memory_manager);
        drop(_system_lock);
        drop(_lock);
        return result;
    }

    pub fn _clone_kernel_memory_if_needed(&mut self) -> Result<(), MemoryError> {
        /* Depend on the architecture */
        if !NEED_COPY_HIGH_MEMORY_PAGE_TABLE {
            return Ok(());
        }
        if self
            .virtual_memory_manager
            .is_kernel_virtual_memory_manager()
        {
            return Ok(());
        }
        assert!(self.lock.is_locked());
        let kernel_memory_manager = &get_kernel_manager_cluster().memory_manager;
        let _system_lock = kernel_memory_manager.lock.lock();
        let result = self
            .virtual_memory_manager
            .clone_kernel_area(&kernel_memory_manager.virtual_memory_manager);
        drop(_system_lock);
        return result;
    }

    pub fn create_user_memory_manager(&self) -> Result<Self, MemoryError> {
        let mut user_virtual_memory_manager = VirtualMemoryManager::new();

        let _lock = self.lock.lock();
        user_virtual_memory_manager
            .init_user(&self.virtual_memory_manager, get_physical_memory_manager())?;
        drop(_lock);
        return Ok(get_kernel_manager_cluster()
            .system_memory_manager
            .create_new_memory_manager(user_virtual_memory_manager));
    }

    fn add_memory_pool_to_physical_memory_manager(
        &mut self,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        if let Err(e) = self
            .virtual_memory_manager
            .add_physical_memory_manager_pool(pm_manager)
        {
            pr_err!(
                "Failed to add memory pool for PhysicalMemoryManager: {:?}",
                e
            );
            return Err(e);
        }
        return Ok(());
    }

    pub fn alloc_pages_with_option(
        &mut self,
        order: MPageOrder,
        permission: MemoryPermissionFlags,
        option: MemoryOptionFlags,
    ) -> Result<VAddress, MemoryError> {
        /* ADD: lazy allocation */
        /* Return physically continuous 2 ^ order pages memory. */
        let size = order.to_offset();
        let _lock = self.lock.lock();
        let pm_manager = get_physical_memory_manager();
        match pm_manager.alloc(size, MOrder::new(PAGE_SHIFT)) {
            Ok(physical_address) => {
                match self.virtual_memory_manager.alloc_and_map_virtual_address(
                    size,
                    physical_address,
                    permission,
                    option,
                    pm_manager,
                ) {
                    Ok(address) => {
                        self._clone_kernel_memory_if_needed()?;
                        Ok(address)
                    }
                    Err(e) => {
                        if let Err(e) = pm_manager.free(physical_address, size, false) {
                            pr_err!("Failed to free physical memory: {:?}", e);
                        }
                        Err(e)
                    }
                }
            }
            Err(MemoryError::EntryPoolRunOut) => {
                if let Err(e) = self
                    .virtual_memory_manager
                    .add_physical_memory_manager_pool(pm_manager)
                {
                    pr_err!(
                        "Failed to add memory pool to PhysicalMemoryManager: {:?}",
                        e
                    );
                    Err(e)
                } else {
                    drop(_lock);
                    self.alloc_pages_with_option(order, permission, option)
                }
            }
            Err(e) => {
                pr_err!("Failed to allocate physical memory: {:?}", e);
                Err(e)
            }
        }
    }

    pub fn alloc_pages(
        &mut self,
        order: MPageOrder,
        permission: MemoryPermissionFlags,
    ) -> Result<VAddress, MemoryError> {
        self.alloc_pages_with_option(order, permission, MemoryOptionFlags::ALLOC)
    }

    pub fn alloc_pages_with_physical_address(
        &mut self,
        order: MPageOrder,
        permission: MemoryPermissionFlags,
    ) -> Result<(VAddress, PAddress), MemoryError> {
        let size = order.to_offset();
        let _lock = self.lock.lock();
        let pm_manager = get_physical_memory_manager();
        match pm_manager.alloc(size, MOrder::new(PAGE_SHIFT)) {
            Ok(physical_address) => {
                match self.virtual_memory_manager.alloc_and_map_virtual_address(
                    size,
                    physical_address,
                    permission,
                    MemoryOptionFlags::ALLOC,
                    pm_manager,
                ) {
                    Ok(virtual_address) => {
                        self._clone_kernel_memory_if_needed()?;
                        Ok((virtual_address, physical_address))
                    }
                    Err(e) => {
                        if let Err(e) = pm_manager.free(physical_address, size, false) {
                            pr_err!("Failed to free physical memory: {:?}", e);
                        }
                        Err(e)
                    }
                }
            }
            Err(MemoryError::EntryPoolRunOut) => {
                if let Err(e) = self
                    .virtual_memory_manager
                    .add_physical_memory_manager_pool(pm_manager)
                {
                    pr_err!(
                        "Failed to add memory pool to PhysicalMemoryManager: {:?}",
                        e
                    );
                    Err(e)
                } else {
                    drop(_lock);
                    self.alloc_pages_with_physical_address(order, permission)
                }
            }
            Err(e) => {
                pr_err!("Failed to allocate physical memory: {:?}", e);
                Err(e)
            }
        }
    }

    /* TODO: check memory access error(sometimes occurs) */
    pub fn alloc_nonlinear_pages(
        &mut self,
        order: MPageOrder,
        permission: MemoryPermissionFlags,
    ) -> Result<VAddress, MemoryError> {
        let size = order.to_offset();
        if size <= PAGE_SIZE {
            return self.alloc_pages(order, permission);
        }
        let _lock = self.lock.lock();

        let entry = self.virtual_memory_manager.alloc_virtual_address(
            size,
            permission,
            MemoryOptionFlags::ALLOC,
        )?;
        let vm_start_address = entry.get_vm_start_address();

        let map_func = |s: &mut Self,
                        i: MIndex,
                        p_a: PAddress,
                        vm_entry: &mut VirtualMemoryEntry,
                        pm_manager: &mut PhysicalMemoryManager|
         -> Result<(), MemoryError> {
            if let Err(e) = s.virtual_memory_manager.map_physical_address_into_vm_entry(
                vm_entry,
                vm_start_address + i.to_offset(),
                p_a,
                PAGE_SIZE,
            ) {
                pr_err!(
                    "Failed to map virtual address({}) to physical address({}): {:?}",
                    vm_start_address + i.to_offset(),
                    p_a,
                    e
                );
                if let Err(e) = s
                    .virtual_memory_manager
                    ._free_address(unsafe { &mut *(vm_entry as *mut _) }, pm_manager)
                {
                    pr_err!("Failed to free vm_entry: {:?}", e);
                }
                return Err(e);
            }
            return Ok(());
        };

        for i in MIndex::new(0)..size.to_index() {
            let pm_manager = get_physical_memory_manager();
            match (*pm_manager).alloc(PAGE_SIZE, MOrder::new(PAGE_SHIFT)) {
                Ok(p_a) => {
                    map_func(self, i, p_a, entry, pm_manager)?;
                }
                Err(MemoryError::EntryPoolRunOut) => {
                    self.add_memory_pool_to_physical_memory_manager(pm_manager)?;
                    match (*pm_manager).alloc(PAGE_SIZE, MOrder::new(PAGE_SHIFT)) {
                        Ok(p_a) => {
                            map_func(self, i, p_a, entry, pm_manager)?;
                        }
                        Err(e) => {
                            pr_err!("Failed to allocate physical memory: {:?}", e);
                            return Err(MemoryError::AllocAddressFailed);
                        }
                    }
                }
                Err(e) => {
                    pr_err!("Failed to allocate physical memory: {:?}", e);
                    return Err(MemoryError::AllocAddressFailed);
                }
            }
        }

        let pm_manager = get_physical_memory_manager();
        if let Err(e) = self
            .virtual_memory_manager
            .update_page_table_with_vm_entry(entry, pm_manager)
        {
            pr_err!("Failed to update page table: {:?}", e);
            if let Err(e) = self.virtual_memory_manager._free_address(entry, pm_manager) {
                pr_err!("Failed to free vm_entry: {:?}", e);
            }
            return Err(MemoryError::PagingError);
        }
        self._clone_kernel_memory_if_needed()?;
        return Ok(vm_start_address);
    }

    pub fn free(&mut self, address: VAddress) -> Result<(), MemoryError> {
        let _lock = self.lock.lock();
        let pm_manager = get_physical_memory_manager();
        let aligned_vm_address = address & PAGE_MASK;
        if let Err(e) = self
            .virtual_memory_manager
            .free_address(aligned_vm_address.into(), pm_manager)
        {
            pr_err!("Failed to free memory: {:?}", e); /* The error of 'free_address' tends to be ignored. */
            return Err(e);
        }
        self._clone_kernel_memory_if_needed()?;
        return Ok(());
        /* Freeing Physical Memory will be done by Virtual Memory Manager, if it be needed. */
    }

    pub fn alloc_physical_memory(&mut self, order: MPageOrder) -> Result<PAddress, MemoryError> {
        /* initializing use only */
        /* Returned memory area is not mapped, if you want to access, you must map. */
        let size = order.to_offset();
        let _lock = self.lock.lock();
        let pm_manager = get_physical_memory_manager();
        match pm_manager.alloc(size, MOrder::new(PAGE_SHIFT)) {
            Ok(p_a) => {
                drop(_lock);
                Ok(p_a)
            }
            Err(MemoryError::EntryPoolRunOut) => {
                self.add_memory_pool_to_physical_memory_manager(pm_manager)?;
                drop(_lock);
                self.alloc_physical_memory(order)
            }
            Err(e) => {
                pr_err!("Failed to allocate PhysicalMemory: {:?}", e);
                drop(_lock);
                Err(MemoryError::AllocAddressFailed)
            }
        }
    }

    pub fn free_physical_memory(
        &mut self,
        address: PAddress,
        size: MSize,
    ) -> Result<(), MemoryError> {
        /* initializing use only */
        let _lock = self.lock.lock();
        if let Err(e) = get_physical_memory_manager().free(address, size, false) {
            drop(_lock);
            pr_err!("Failed to free physical memory: {:?}", e);
            Err(e)
        } else {
            drop(_lock);
            Ok(())
        }
    }

    pub fn mmap(
        &mut self,
        physical_address: PAddress,
        size: MSize,
        permission: MemoryPermissionFlags,
        option: MemoryOptionFlags,
    ) -> Result<VAddress, MemoryError> {
        /* For some data mapping */
        /* should remake...? */
        let (aligned_physical_address, aligned_size) = Self::page_align(physical_address, size);
        let _lock = self.lock.lock();

        let pm_manager = get_physical_memory_manager();

        if !option.is_pre_reserved() {
            if let Err(e) =
                pm_manager.reserve_memory(aligned_physical_address, size, MOrder::new(0))
            {
                return if e == MemoryError::EntryPoolRunOut {
                    self.add_memory_pool_to_physical_memory_manager(pm_manager)?;
                    drop(_lock);
                    self.mmap(physical_address, size, permission, option)
                } else {
                    drop(_lock);
                    pr_err!("Failed to reserve physical memory: {:?}", e);
                    Err(MemoryError::AllocAddressFailed)
                };
            }
        }
        let virtual_address = self.virtual_memory_manager.map_address(
            aligned_physical_address,
            None,
            aligned_size,
            permission,
            option | MemoryOptionFlags::MEMORY_MAP,
            pm_manager,
        )?;

        self._clone_kernel_memory_if_needed()?;
        drop(physical_address);
        drop(_lock);
        Ok(virtual_address + (physical_address - aligned_physical_address))
    }

    pub fn io_map(
        &mut self,
        physical_address: PAddress,
        size: MSize,
        permission: MemoryPermissionFlags,
        option: Option<MemoryOptionFlags>,
    ) -> Result<VAddress, MemoryError> {
        /* For IO map */
        /* should remake...? */
        let (aligned_physical_address, aligned_size) = Self::page_align(physical_address, size);
        let _lock = self.lock.lock();

        let pm_manager = get_physical_memory_manager();
        //pm_manager.reserve_memory(aligned_physical_address, size, false);
        /* physical_address must be reserved. */
        /* ADD: check succeeded or failed (failed because of already reserved is ok, but other...) */
        let virtual_address = self.virtual_memory_manager.io_map(
            aligned_physical_address,
            None,
            aligned_size,
            permission,
            option,
            pm_manager,
        )?;
        self._clone_kernel_memory_if_needed()?;
        drop(_lock);
        Ok(virtual_address + (physical_address - aligned_physical_address))
    }

    pub fn mremap_dev(
        &mut self,
        old_virtual_address: VAddress,
        _old_size: MSize,
        new_size: MSize,
    ) -> Result<VAddress, MemoryError> {
        let (aligned_virtual_address, aligned_new_size) =
            Self::page_align(old_virtual_address, new_size);

        let _lock = self.lock.lock();
        let pm_manager = get_physical_memory_manager();

        //pm_manager.reserve_memory(aligned_physical_address, size, false);
        /* physical_address must be reserved. */

        let new_virtual_address = self.virtual_memory_manager.resize_memory_mapping(
            aligned_virtual_address,
            aligned_new_size,
            pm_manager,
        )?;

        self._clone_kernel_memory_if_needed()?;
        drop(_lock);
        Ok(new_virtual_address + (old_virtual_address - aligned_virtual_address))
    }

    pub fn set_paging_table(&mut self) {
        let _lock = self.lock.lock();
        self.virtual_memory_manager.flush_paging();
    }

    pub fn dump_memory_manager(&self) {
        kprintln!("----Physical Memory Entries Dump----");
        if let Err(_) = get_physical_memory_manager().dump_memory_entry() {
            kprintln!("Failed to dump Physical Memory Manager");
        }
        kprintln!("----Physical Memory Entries Dump End----");
        kprintln!("----Virtual Memory Entries Dump----");
        self.virtual_memory_manager.dump_memory_manager(None, None);
        kprintln!("----Virtual Memory Entries Dump End----");
    }

    #[inline] /* want to be const... */
    pub fn page_align<T: Address>(address: T, size: MSize) -> (T /*address*/, MSize /*size*/) {
        if size.is_zero() && (address.to_usize() & PAGE_MASK) == 0 {
            (address, MSize::new(0))
        } else {
            (
                (address.to_usize() & PAGE_MASK).into(),
                MSize::new(
                    (size.to_usize() + (address.to_usize() - (address.to_usize() & PAGE_MASK)) - 1)
                        & PAGE_MASK,
                ) + PAGE_SIZE,
            )
        }
    }
}
