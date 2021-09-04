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
use self::virtual_memory_manager::{VirtualMemoryEntry, VirtualMemoryManager};

use crate::arch::target_arch::paging::{PAGE_MASK, PAGE_SHIFT, PAGE_SIZE};

use crate::kernel::sync::spin_lock::{IrqSaveSpinLockFlag, Mutex};

pub struct MemoryManager {
    lock: IrqSaveSpinLockFlag,
    physical_memory_manager: &'static Mutex<PhysicalMemoryManager>,
    virtual_memory_manager: VirtualMemoryManager,
}

/* To share PhysicalMemoryManager */
pub struct SystemMemoryManager {
    original_physical_memory_manager: Mutex<PhysicalMemoryManager>,
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
    pub const fn new(physical_memory_manager: PhysicalMemoryManager) -> Self {
        Self {
            original_physical_memory_manager: Mutex::new(physical_memory_manager),
        }
    }

    pub fn create_new_memory_manager(
        &'static self,
        virtual_memory_manager: VirtualMemoryManager,
    ) -> MemoryManager {
        MemoryManager::new(
            &self.original_physical_memory_manager,
            virtual_memory_manager,
        )
    }
}

impl MemoryManager {
    pub fn new(
        physical_memory_manager: &'static Mutex<PhysicalMemoryManager>,
        virtual_memory_manager: VirtualMemoryManager,
    ) -> Self {
        Self {
            physical_memory_manager,
            virtual_memory_manager,
            lock: IrqSaveSpinLockFlag::new(),
        }
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

    pub fn alloc_pages(
        &mut self,
        order: MPageOrder,
        permission: MemoryPermissionFlags,
    ) -> Result<VAddress, MemoryError> {
        /* ADD: lazy allocation */
        /* Return physically continuous 2 ^ order pages memory. */
        let size = order.to_offset();
        let _lock = self.lock.lock();
        let mut pm_manager = self.physical_memory_manager.lock().unwrap();
        match pm_manager.alloc(size, MOrder::new(PAGE_SHIFT)) {
            Ok(physical_address) => {
                match self.virtual_memory_manager.alloc_and_map_virtual_address(
                    size,
                    physical_address,
                    permission,
                    MemoryOptionFlags::ALLOC,
                    &mut pm_manager,
                ) {
                    Ok(address) => Ok(address),
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
                    .add_physical_memory_manager_pool(&mut pm_manager)
                {
                    pr_err!(
                        "Failed to add memory pool to PhysicalMemoryManager: {:?}",
                        e
                    );
                    Err(e)
                } else {
                    drop(pm_manager);
                    drop(_lock);
                    self.alloc_pages(order, permission)
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

        let mut pm_manager = self.physical_memory_manager.lock().unwrap();
        let entry = self.virtual_memory_manager.alloc_virtual_address(
            size,
            permission,
            MemoryOptionFlags::ALLOC,
            &mut pm_manager,
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
                pm_manager,
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
            match pm_manager.alloc(PAGE_SIZE, MOrder::new(PAGE_SHIFT)) {
                Ok(p_a) => {
                    map_func(self, i, p_a, entry, &mut pm_manager)?;
                }
                Err(MemoryError::EntryPoolRunOut) => {
                    self.add_memory_pool_to_physical_memory_manager(&mut pm_manager)?;
                    match pm_manager.alloc(PAGE_SIZE, MOrder::new(PAGE_SHIFT)) {
                        Ok(p_a) => {
                            map_func(self, i, p_a, entry, &mut pm_manager)?;
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
        if let Err(e) = self
            .virtual_memory_manager
            .update_page_table_with_vm_entry(entry, &mut pm_manager)
        {
            pr_err!("Failed to update page table: {:?}", e);
            if let Err(e) = self
                .virtual_memory_manager
                ._free_address(entry, &mut pm_manager)
            {
                pr_err!("Failed to free vm_entry: {:?}", e);
            }
            return Err(MemoryError::PagingError);
        }
        return Ok(vm_start_address);
    }

    pub fn alloc_with_option(
        &mut self,
        order: MPageOrder,
        permission: MemoryPermissionFlags,
        option: MemoryOptionFlags,
    ) -> Result<VAddress, MemoryError> {
        let size = order.to_offset();

        if option.is_direct_mapped() && permission.is_executable() == false {
            let _lock = self.lock.lock();
            let mut physical_memory_manager = self.physical_memory_manager.lock().unwrap();
            return self
                .virtual_memory_manager
                .alloc_from_direct_map(size, &mut physical_memory_manager);
        }
        unimplemented!()
    }

    pub fn free(&mut self, address: VAddress) -> Result<(), MemoryError> {
        let _lock = self.lock.lock();
        let mut pm_manager = self.physical_memory_manager.lock().unwrap();
        let aligned_vm_address = address & PAGE_MASK;
        if let Err(e) = self
            .virtual_memory_manager
            .free_address(aligned_vm_address.into(), &mut pm_manager)
        {
            pr_err!("Failed to free memory: {:?}", e); /* The error of 'free_address' tends to be ignored. */
            Err(e)
        } else {
            Ok(())
        }
        /* Freeing Physical Memory will be done by Virtual Memory Manager, if it be needed. */
    }

    pub fn alloc_physical_memory(&mut self, order: MPageOrder) -> Result<PAddress, MemoryError> {
        /* initializing use only */
        /* Returned memory area is not mapped, if you want to access, you must map. */
        let size = order.to_offset();
        let _lock = self.lock.lock();
        let mut pm_manager = self.physical_memory_manager.lock().unwrap();
        match pm_manager.alloc(size, MOrder::new(PAGE_SHIFT)) {
            Ok(p_a) => {
                drop(pm_manager);
                drop(_lock);
                Ok(p_a)
            }
            Err(MemoryError::EntryPoolRunOut) => {
                self.add_memory_pool_to_physical_memory_manager(&mut pm_manager)?;
                drop(pm_manager);
                drop(_lock);
                self.alloc_physical_memory(order)
            }
            Err(e) => {
                pr_err!("Failed to allocate PhysicalMemory: {:?}", e);
                drop(pm_manager);
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
        if let Err(e) = self
            .physical_memory_manager
            .lock()
            .unwrap()
            .free(address, size, false)
        {
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

        let mut pm_manager = self.physical_memory_manager.lock().unwrap();

        if !option.is_pre_reserved() {
            if let Err(e) =
                pm_manager.reserve_memory(aligned_physical_address, size, MOrder::new(0))
            {
                return if e == MemoryError::EntryPoolRunOut {
                    self.add_memory_pool_to_physical_memory_manager(&mut pm_manager)?;
                    drop(pm_manager);
                    drop(_lock);
                    self.mmap(physical_address, size, permission, option)
                } else {
                    drop(pm_manager);
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
            &mut pm_manager,
        )?;

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

        let mut pm_manager = self.physical_memory_manager.lock().unwrap();
        //pm_manager.reserve_memory(aligned_physical_address, size, false);
        /* physical_address must be reserved. */
        /* ADD: check succeeded or failed (failed because of already reserved is ok, but other...) */
        let virtual_address = self.virtual_memory_manager.io_map(
            aligned_physical_address,
            None,
            aligned_size,
            permission,
            option,
            &mut pm_manager,
        )?;
        drop(pm_manager);
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
        let mut pm_manager = self.physical_memory_manager.lock().unwrap();

        //pm_manager.reserve_memory(aligned_physical_address, size, false);
        /* physical_address must be reserved. */

        let new_virtual_address = self.virtual_memory_manager.resize_memory_mapping(
            aligned_virtual_address,
            aligned_new_size,
            &mut pm_manager,
        )?;

        drop(pm_manager);
        drop(_lock);
        Ok(new_virtual_address + (old_virtual_address - aligned_virtual_address))
    }

    pub fn set_paging_table(&mut self) {
        let _lock = self.lock.lock();
        self.virtual_memory_manager.flush_paging();
    }

    pub fn dump_memory_manager(&self) {
        if let Ok(physical_memory_manager) = self.physical_memory_manager.try_lock() {
            kprintln!("----Physical Memory Entries Dump----");
            physical_memory_manager.dump_memory_entry();
            kprintln!("----Physical Memory Entries Dump End----");
        } else {
            kprintln!("Can not lock Physical Memory Manager.");
        }
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
