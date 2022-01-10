//!
//! Memory Manager
//!
//! This manager is the frontend of the memory allocation.
//! Each process has a memory manager.
//! MemoryManager treats page size allocation, if you want to alloc memory for objects in the kernel,
//! use memory_allocator.
//! In this memory system, you should not use alloc::*, use only core::*
//!

pub mod data_type;
pub mod global_allocator;
pub mod memory_allocator;
pub mod physical_memory_manager;
pub mod slab_allocator;
pub mod system_memory_manager;
pub mod virtual_memory_manager;

use self::data_type::{
    Address, MIndex, MOrder, MPageOrder, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress,
    VAddress,
};
use self::physical_memory_manager::PhysicalMemoryManager;
use self::system_memory_manager::get_physical_memory_manager;
use self::virtual_memory_manager::{VirtualMemoryEntry, VirtualMemoryManager};
use crate::arch::target_arch::paging::{
    NEED_COPY_HIGH_MEMORY_PAGE_TABLE, PAGE_MASK, PAGE_SHIFT, PAGE_SIZE,
};

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::system_memory_manager::SystemMemoryManager;
use crate::kernel::sync::spin_lock::IrqSaveSpinLockFlag;

pub struct MemoryManager {
    lock: IrqSaveSpinLockFlag,
    virtual_memory_manager: VirtualMemoryManager,
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

impl MemoryManager {
    pub fn new(virtual_memory_manager: VirtualMemoryManager) -> Self {
        Self {
            virtual_memory_manager,
            lock: IrqSaveSpinLockFlag::new(),
        }
    }

    pub fn disable(&mut self) {
        assert!(!self.is_kernel_memory_manager());
        let _lock = self.lock.lock();
        self.virtual_memory_manager.disable();
    }

    pub fn is_kernel_memory_manager(&self) -> bool {
        &get_kernel_manager_cluster().kernel_memory_manager as *const _ == self as *const _
    }

    fn clone_kernel_memory_pages(&mut self) -> Result<(), MemoryError> {
        let kernel_memory_manager = &get_kernel_manager_cluster().kernel_memory_manager;
        let _kernel_memory_manager_lock = kernel_memory_manager.lock.lock();
        let result = self
            .virtual_memory_manager
            .clone_kernel_area(&kernel_memory_manager.virtual_memory_manager);
        drop(_kernel_memory_manager_lock);
        return result;
    }

    pub fn clone_kernel_memory_pages_if_needed(&mut self) -> Result<(), MemoryError> {
        /* Depend on the architecture */
        if !NEED_COPY_HIGH_MEMORY_PAGE_TABLE {
            return Ok(());
        }
        if self.is_kernel_memory_manager() {
            return Ok(());
        }
        let _lock = self.lock.lock();
        let result = self.clone_kernel_memory_pages();
        drop(_lock);
        return result;
    }

    fn _clone_kernel_memory_pages_if_needed(&mut self) -> Result<(), MemoryError> {
        /* Depend on the architecture */
        if !NEED_COPY_HIGH_MEMORY_PAGE_TABLE {
            return Ok(());
        }
        if self.is_kernel_memory_manager() {
            return Ok(());
        }
        assert!(self.lock.is_locked());
        let result = self.clone_kernel_memory_pages();
        return result;
    }

    pub fn create_user_memory_manager(&self) -> Result<Self, MemoryError> {
        assert!(self.is_kernel_memory_manager());
        let mut user_virtual_memory_manager = VirtualMemoryManager::new();

        let _lock = self.lock.lock();
        user_virtual_memory_manager
            .init_user(&self.virtual_memory_manager, get_physical_memory_manager())?;
        drop(_lock);
        return Ok(Self::new(user_virtual_memory_manager));
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

    fn _alloc_pages(
        &mut self,
        order: MPageOrder,
        permission: MemoryPermissionFlags,
        option: MemoryOptionFlags,
    ) -> Result<(VAddress, PAddress), MemoryError> {
        /* ADD: lazy allocation */
        /* Return physically continuous 2 ^ order pages memory. */
        let size = order.to_offset();
        let mut _lock = self.lock.lock();
        let pm_manager = get_physical_memory_manager();
        let physical_address = {
            let p: PAddress;
            loop {
                p = match pm_manager.alloc(size, MOrder::new(PAGE_SHIFT)) {
                    Ok(physical_address) => physical_address,
                    Err(MemoryError::EntryPoolRunOut) => {
                        if let Err(e) = self
                            .virtual_memory_manager
                            .add_physical_memory_manager_pool(pm_manager)
                        {
                            pr_err!(
                                "Failed to add memory pool to PhysicalMemoryManager: {:?}",
                                e
                            );
                            drop(_lock);
                            return Err(e);
                        }
                        continue;
                    }
                    Err(e) => {
                        drop(_lock);
                        pr_err!("Failed to allocate physical memory: {:?}", e);
                        return Err(e);
                    }
                };
                break;
            }
            p
        };
        loop {
            match self.virtual_memory_manager.alloc_and_map_virtual_address(
                size,
                physical_address,
                permission,
                option,
                pm_manager,
            ) {
                Ok(address) => {
                    self._clone_kernel_memory_pages_if_needed()?;
                    return Ok((address, physical_address));
                }
                Err(MemoryError::EntryPoolRunOut) => {
                    if option.is_no_wait() {
                        return Err(MemoryError::EntryPoolRunOut);
                    }
                    drop(_lock);
                    self.add_entry_pool()?;
                    _lock = self.lock.lock();

                    continue;
                }
                Err(e) => {
                    if let Err(e) = pm_manager.free(physical_address, size, false) {
                        pr_err!("Failed to free physical memory: {:?}", e);
                    }
                    return Err(e);
                }
            }
        }
    }

    pub fn alloc_pages_with_physical_address(
        &mut self,
        order: MPageOrder,
        permission: MemoryPermissionFlags,
        option: Option<MemoryOptionFlags>,
    ) -> Result<(VAddress, PAddress), MemoryError> {
        self._alloc_pages(
            order,
            permission,
            option.unwrap_or(MemoryOptionFlags::KERNEL) | MemoryOptionFlags::ALLOC,
        )
    }

    pub fn alloc_pages(
        &mut self,
        order: MPageOrder,
        permission: MemoryPermissionFlags,
        option: Option<MemoryOptionFlags>,
    ) -> Result<VAddress, MemoryError> {
        self.alloc_pages_with_physical_address(order, permission, option)
            .and_then(|r| Ok(r.0))
    }

    /* TODO: check memory access error(sometimes occurs) */
    /* TODO: rewrite(For entry pool allocation) */
    pub fn alloc_nonlinear_pages(
        &mut self,
        order: MPageOrder,
        permission: MemoryPermissionFlags,
        option: Option<MemoryOptionFlags>,
    ) -> Result<VAddress, MemoryError> {
        let size = order.to_offset();
        if size <= PAGE_SIZE {
            return self.alloc_pages(order, permission, option);
        }
        let _lock = self.lock.lock();

        let entry = self.virtual_memory_manager.alloc_virtual_address(
            size,
            permission,
            option.unwrap_or(MemoryOptionFlags::KERNEL | MemoryOptionFlags::ALLOC),
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
        self._clone_kernel_memory_pages_if_needed()?;
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
        self._clone_kernel_memory_pages_if_needed()?;
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

    pub fn io_remap(
        &mut self,
        physical_address: PAddress,
        size: MSize,
        permission: MemoryPermissionFlags,
        option: Option<MemoryOptionFlags>,
    ) -> Result<VAddress, MemoryError> {
        let (aligned_physical_address, aligned_size) = Self::page_align(physical_address, size);
        let mut _lock = self.lock.lock();

        let pm_manager = get_physical_memory_manager();
        /* TODO: check physical_address is not allocatble */
        let option = option.unwrap_or(MemoryOptionFlags::KERNEL)
            | MemoryOptionFlags::IO_MAP
            | MemoryOptionFlags::DEVICE_MEMORY;
        let virtual_address = {
            let v: VAddress;
            loop {
                v = match self.virtual_memory_manager.map_address(
                    aligned_physical_address,
                    None,
                    aligned_size,
                    permission,
                    option,
                    pm_manager,
                ) {
                    Ok(v) => v,
                    Err(MemoryError::EntryPoolRunOut) => {
                        if option.is_no_wait() {
                            return Err(MemoryError::EntryPoolRunOut);
                        }
                        if self.is_kernel_memory_manager() {
                            drop(_lock);
                            self.add_entry_pool()?;
                            _lock = self.lock.lock();
                        } else {
                            self.add_entry_pool()?;
                        }
                        continue;
                    }
                    Err(e) => {
                        drop(_lock);
                        return Err(e);
                    }
                };
                break;
            }
            v
        };

        self._clone_kernel_memory_pages_if_needed()?;
        drop(_lock);
        Ok(virtual_address + (physical_address - aligned_physical_address))
    }

    pub fn mremap(
        &mut self,
        old_virtual_address: VAddress,
        _old_size: MSize,
        new_size: MSize,
    ) -> Result<VAddress, MemoryError> {
        let (aligned_virtual_address, aligned_new_size) =
            Self::page_align(old_virtual_address, new_size);

        let mut _lock = self.lock.lock();
        let pm_manager = get_physical_memory_manager();

        //pm_manager.reserve_memory(aligned_physical_address, size, false);
        /* physical_address must be reserved. */

        let new_virtual_address = {
            let v: VAddress;
            loop {
                v = match self.virtual_memory_manager.resize_memory_mapping(
                    aligned_virtual_address,
                    aligned_new_size,
                    pm_manager,
                ) {
                    Ok(v) => v,
                    Err(MemoryError::EntryPoolRunOut) => {
                        drop(_lock);
                        self.add_entry_pool()?;
                        _lock = self.lock.lock();
                        continue;
                    }
                    Err(e) => {
                        drop(_lock);
                        return Err(e);
                    }
                };
                break;
            }
            v
        };

        self._clone_kernel_memory_pages_if_needed()?;
        drop(_lock);
        Ok(new_virtual_address + (old_virtual_address - aligned_virtual_address))
    }

    fn add_entry_pool(&mut self) -> Result<(), MemoryError> {
        SystemMemoryManager::pool_alloc_worker(
            SystemMemoryManager::ALLOC_VM_ENTRY_FLAG | SystemMemoryManager::ALLOC_VM_PAGE_FLAG,
        );
        return Ok(());
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

#[macro_export]
macro_rules! io_remap {
    ($address:expr, $len:expr, $permission:expr) => {
        crate::kernel::manager_cluster::get_kernel_manager_cluster()
            .kernel_memory_manager
            .io_remap($address, $len, $permission, None)
    };
    ($address:expr, $len:expr, $permission:expr,$option:expr) => {
        crate::kernel::manager_cluster::get_kernel_manager_cluster()
            .kernel_memory_manager
            .io_remap($address, $len, $permission, Some($option))
    };
}

#[macro_export]
macro_rules! mremap {
    ($old_address:expr, $old_size:expr, $new_size:expr) => {
        crate::kernel::manager_cluster::get_kernel_manager_cluster()
            .kernel_memory_manager
            .mremap($old_address, $old_size, $new_size)
    };
}

#[macro_export]
macro_rules! alloc_pages {
    ($order:expr, $permission:expr) => {
        crate::kernel::manager_cluster::get_kernel_manager_cluster()
            .kernel_memory_manager
            .alloc_pages($order, $permission, None)
    };
    ($order:expr, $permission:expr, $option:expr) => {
        crate::kernel::manager_cluster::get_kernel_manager_cluster()
            .kernel_memory_manager
            .alloc_pages($order, $permission, Some($option))
    };
}

#[macro_export]
macro_rules! alloc_pages_with_physical_address {
    ($order:expr, $permission:expr) => {
        crate::kernel::manager_cluster::get_kernel_manager_cluster()
            .kernel_memory_manager
            .alloc_pages_with_physical_address($order, $permission, None)
    };
    ($order:expr, $permission:expr, $option:expr) => {
        crate::kernel::manager_cluster::get_kernel_manager_cluster()
            .kernel_memory_manager
            .alloc_pages_with_physical_address($order, $permission, Some($option))
    };
}

#[macro_export]
macro_rules! alloc_non_linear_pages {
    ($order:expr, $permission:expr) => {
        crate::kernel::manager_cluster::get_kernel_manager_cluster()
            .kernel_memory_manager
            .alloc_nonlinear_pages($order, $permission, None)
    };
    ($order:expr, $permission:expr, $option:expr) => {
        crate::kernel::manager_cluster::get_kernel_manager_cluster()
            .kernel_memory_manager
            .alloc_nonlinear_pages($order, $permission, Some($option))
    };
}

#[macro_export]
macro_rules! kmalloc {
    ($size:expr) => {
        crate::kernel::manager_cluster::get_cpu_manager_cluster()
            .memory_allocator
            .kmalloc($size)
    };

    ($t:ty, $initial_value:expr) => {
        crate::kernel::manager_cluster::get_cpu_manager_cluster()
            .memory_allocator
            .kmalloc(crate::kernel::memory_manager::data_type::MSize::new(
                core::mem::size_of::<$t>(),
            ))
            .and_then(|addr| {
                let o = unsafe { &mut *(addr.to_usize() as *mut $t) };
                core::mem::forget(core::mem::replace(o, $initial_value));
                Ok(o)
            })
    };
}
