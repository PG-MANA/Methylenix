//!
//! Virtual Memory Manager
//!
//! This manager maintains memory mapping and controls page_manager.
//! The address and size are rounded up to an integral number of PAGE_SIZE.
//!

/* ADD: add physical_memory into reserved_memory_list when it runs out */

mod virtual_memory_entry;
mod virtual_memory_object;
mod virtual_memory_page;

pub(super) use self::virtual_memory_entry::VirtualMemoryEntry;
pub(super) use self::virtual_memory_object::VirtualMemoryObject;
pub(super) use self::virtual_memory_page::VirtualMemoryPage;

use super::{
    MemoryError, MemoryManager, data_type::*, physical_memory_manager::PhysicalMemoryManager,
    system_memory_manager::SystemMemoryManager,
};

use crate::arch::target_arch::context::memory_layout::*;
use crate::arch::target_arch::paging::*;

use crate::kernel::collections::init_struct;
use crate::kernel::collections::ptr_linked_list::PtrLinkedList;
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::sync::spin_lock::ClassicIrqSaveSpinLockFlag;

use core::mem::offset_of;
use core::ops::RangeInclusive;

pub struct VirtualMemoryManager {
    lock: ClassicIrqSaveSpinLockFlag,
    vm_entry: PtrLinkedList<VirtualMemoryEntry>,
    page_manager: PageManager,
}

macro_rules! find_vm_entry {
    ($s:expr,$addr:expr) => {
        unsafe { $s.vm_entry.iter(offset_of!(VirtualMemoryEntry, list)) }
            .find(|&e| e.get_vm_start_address() <= $addr && e.get_vm_end_address() >= $addr)
    };
}

macro_rules! find_vm_entry_mut {
    ($s:expr,$addr:expr) => {
        unsafe { $s.vm_entry.iter_mut(offset_of!(VirtualMemoryEntry, list)) }
            .find(|e| e.get_vm_start_address() <= $addr && e.get_vm_end_address() >= $addr)
            .map(|e| unsafe { &mut *(e as *mut VirtualMemoryEntry) })
    };
}

macro_rules! find_previous_vm_entry_mut {
    ($s:expr,$addr:expr) => {{
        let mut prev = None;
        const OFFSET: usize = offset_of!(VirtualMemoryEntry, list);
        for e in unsafe { $s.vm_entry.iter_mut(OFFSET) } {
            if e.get_vm_start_address() > $addr {
                prev = e.list.get_prev_mut(OFFSET).map(|e| unsafe { &mut *e });
                break;
            } else if !e.list.has_next() && e.get_vm_end_address() < $addr {
                prev = Some(unsafe { &mut *(e as *mut VirtualMemoryEntry) });
            }
        }
        prev
    }};
}

impl Default for VirtualMemoryManager {
    fn default() -> Self {
        Self::new()
    }
}

impl VirtualMemoryManager {
    pub const fn new() -> Self {
        Self {
            lock: ClassicIrqSaveSpinLockFlag::new(),
            vm_entry: PtrLinkedList::new(),
            page_manager: PageManager::new(),
        }
    }

    pub fn is_kernel_virtual_memory_manager(&self) -> bool {
        core::ptr::eq(
            self,
            &get_kernel_manager_cluster()
                .kernel_memory_manager
                .virtual_memory_manager,
        )
    }

    pub fn clone_kernel_area(
        &mut self,
        kernel_virtual_memory_manager: &Self,
    ) -> Result<(), MemoryError> {
        assert!(!self.is_kernel_virtual_memory_manager());
        kernel_virtual_memory_manager.lock.lock();
        self.lock.lock();
        if let Err(e) = self
            .page_manager
            .copy_system_area(&kernel_virtual_memory_manager.page_manager)
        {
            self.lock.unlock();
            kernel_virtual_memory_manager.lock.unlock();
            pr_err!("Failed to copy kernel area: {:?}", e);
            return Err(MemoryError::PagingError(e));
        }
        self.lock.unlock();
        kernel_virtual_memory_manager.lock.unlock();
        Ok(())
    }

    pub fn init_system(&mut self, pm_manager: &mut PhysicalMemoryManager) {
        self.lock.lock();
        /* Set up page_manager */
        self.page_manager
            .init(pm_manager)
            .expect("Cannot init PageManager");

        self.setup_direct_mapped_area(pm_manager);
        self.lock.unlock();
    }

    pub fn init_user(
        &mut self,
        system_virtual_memory_manager: &VirtualMemoryManager,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        self.lock.lock();
        /* Set up page_manager */
        if let Err(e) = self
            .page_manager
            .init_user(&system_virtual_memory_manager.page_manager, pm_manager)
        {
            self.lock.unlock();
            pr_err!("Failed to init PageManager for user: {:?}", e);
            return Err(MemoryError::PagingError(e));
        }
        self.lock.unlock();
        Ok(())
    }

    #[inline]
    fn check_align(
        physical_address: Option<PAddress>,
        virtual_address: Option<VAddress>,
        size: Option<MSize>,
    ) -> Result<(), MemoryError> {
        if let Some(p) = physical_address
            && p & !PAGE_MASK != 0
        /* Physical Address allows zero */
        {
            pr_err!("Physical Address({p}) is not aligned.");
            return Err(MemoryError::NotAligned);
        }
        if let Some(v) = virtual_address
            && (v.is_zero() || v & !PAGE_MASK != 0)
        {
            pr_err!("Virtual Address({v}) is zero or not aligned.");
            return Err(MemoryError::NotAligned);
        }
        if let Some(s) = size
            && (s.is_zero() || (s & !PAGE_MASK != 0))
        {
            pr_err!("Size({s}) is zero or not aligned.");
            return Err(MemoryError::InvalidSize);
        }
        Ok(())
    }

    fn setup_direct_mapped_area(&mut self, pm_manager: &mut PhysicalMemoryManager) {
        assert!(self.lock.is_locked());
        let start_virtual_address = get_direct_map_start_address();
        let start_physical_address = get_direct_map_base_address();
        let map_size = get_direct_map_size();
        pr_info!(
            "DMA: [{:#016X} ~ {:#016X}] => [{:#016X} ~ {:#016X}]",
            start_virtual_address.to_usize(),
            map_size.to_end_address(start_virtual_address).to_usize(),
            start_physical_address.to_usize(),
            map_size.to_end_address(start_physical_address).to_usize(),
        );
        self.map_address_into_page_table(
            start_physical_address,
            start_virtual_address,
            map_size,
            MemoryPermissionFlags::new(true, true, true, false),
            MemoryOptionFlags::KERNEL | MemoryOptionFlags::ALLOW_HUGE,
            pm_manager,
        )
        .expect("Failed to map physical memory");
        self._update_paging_all();
    }

    pub fn flush_paging(&mut self) {
        self.lock.lock();
        self.page_manager.flush_page_table();
        self.lock.unlock();
    }

    fn _update_paging(&self, address: VAddress, range: MSize) {
        self.page_manager.update_page_cache(address, range);
    }

    pub fn update_paging(&self, address: VAddress, range: MSize) {
        self.lock.lock();
        self._update_paging(address, range);
        self.lock.unlock();
    }

    fn _update_paging_all(&self) {
        PageManager::update_page_cache_all();
    }

    fn _alloc_virtual_address(
        &mut self,
        size: MSize,
        permission: MemoryPermissionFlags,
        option: MemoryOptionFlags,
    ) -> Result<&'static mut VirtualMemoryEntry, MemoryError> {
        assert!(self.lock.is_locked());
        // Allocate vm_entry to reserve memory area
        let vm_entry = self.alloc_vm_entry(option)?;

        // Find available memory area
        let (virtual_address_limit_start, virtual_address_limit_end) = if option.is_for_kernel() {
            if option.is_io_map() {
                (MAP_START_ADDRESS, MAP_END_ADDRESS)
            } else if option.is_alloc_area() {
                (MALLOC_START_ADDRESS, MALLOC_END_ADDRESS)
            } else {
                unimplemented!("Unimplemented option");
            }
        } else {
            if option.is_stack() {
                (USER_STACK_START_ADDRESS, USER_STACK_END_ADDRESS)
            } else {
                (USER_START_ADDRESS, USER_END_ADDRESS)
            }
        };
        const OFFSET: usize = offset_of!(VirtualMemoryEntry, list);
        let mut available_start_address = virtual_address_limit_start;

        for e in unsafe { self.vm_entry.iter(OFFSET) } {
            if e.get_vm_end_address() < virtual_address_limit_start {
                continue;
            }
            let end_address = size.to_end_address(available_start_address);
            if end_address > virtual_address_limit_end {
                get_kernel_manager_cluster()
                    .system_memory_manager
                    .free_vm_entry(vm_entry);
                return Err(MemoryError::AddressNotAvailable);
            }
            if !Self::is_overlapped(
                &(available_start_address..=end_address),
                &(e.get_vm_start_address()..=e.get_vm_end_address()),
            ) {
                assert!(
                    e.list
                        .get_next(OFFSET)
                        .map(|n| {
                            let n = unsafe { &*n };
                            !Self::is_overlapped(
                                &(available_start_address..=end_address),
                                &(n.get_vm_start_address()..=n.get_vm_end_address()),
                            )
                        })
                        .unwrap_or(true)
                );
                assert!(
                    e.list
                        .get_prev(OFFSET)
                        .map(|p| {
                            let p = unsafe { &*p };
                            !Self::is_overlapped(
                                &(available_start_address..=end_address),
                                &(p.get_vm_start_address()..=p.get_vm_end_address()),
                            )
                        })
                        .unwrap_or(true)
                );
                break;
            }
            available_start_address = e.get_vm_end_address() + MSize::new(1);
        }
        let end_address = size.to_end_address(available_start_address);
        if end_address > virtual_address_limit_end {
            get_kernel_manager_cluster()
                .system_memory_manager
                .free_vm_entry(vm_entry);
            return Err(MemoryError::AddressNotAvailable);
        }

        // Insert vm_entry to reserve memory area
        init_struct!(
            *vm_entry,
            VirtualMemoryEntry::new(available_start_address, end_address, permission, option)
        );
        self.insert_vm_map_entry_into_list(vm_entry)
            .inspect_err(|_| {
                get_kernel_manager_cluster()
                    .system_memory_manager
                    .free_vm_entry(vm_entry)
            })?;
        Ok(vm_entry)
    }

    /// Allocate the available virtual address and return inserted VirtualMemoryEntry
    ///
    /// This function will search available virtual address and
    /// reserve the range of from the found virtual address to the size.
    /// This will be used to map non-linear memory or lazy mapping.
    pub(super) fn alloc_virtual_address(
        &mut self,
        size: MSize,
        permission: MemoryPermissionFlags,
        option: MemoryOptionFlags,
    ) -> Result<&'static mut VirtualMemoryEntry, MemoryError> {
        Self::check_align(None, None, Some(size))?;
        self.lock.lock();
        let result = self._alloc_virtual_address(size, permission, option);
        self.lock.unlock();
        result
    }

    /// Map virtual_address to physical_address with size.
    ///
    /// This function maps virtual_address to physical_address into vm_entry.
    /// vm_entry must be inserted in [`Self::vm_entry`]. (use the entry from [`Self::alloc_virtual_address`])
    /// virtual_address, physical_address, and size must be page aligned.
    /// This function also map the page into page table.
    pub(super) fn map_physical_address_into_vm_entry_and_page_table(
        &mut self,
        vm_entry: &mut VirtualMemoryEntry,
        virtual_address: VAddress,
        physical_address: PAddress,
        size: MSize,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        Self::check_align(Some(physical_address), Some(virtual_address), Some(size))?;
        let vm_entry_range = vm_entry.get_vm_start_address()..=vm_entry.get_vm_end_address();
        if !vm_entry_range.contains(&virtual_address)
            || !vm_entry_range.contains(&size.to_end_address(virtual_address))
        {
            pr_err!(
                "AddressRange({} ~ {}) is out of vm_entry({:?}).",
                virtual_address,
                size.to_end_address(virtual_address),
                vm_entry_range
            );
            return Err(MemoryError::InvalidAddress);
        }
        self.lock.lock();
        let result = self.map_address_into_page_table(
            physical_address,
            virtual_address,
            size,
            vm_entry.get_permission_flags(),
            vm_entry.get_memory_option_flags(),
            pm_manager,
        );
        if let Err(e) = result {
            self.lock.unlock();
            pr_err!("Failed to map memory: {:?}", e);
            return result;
        }
        let result = self.insert_pages_into_vm_entry(
            vm_entry,
            physical_address,
            virtual_address,
            size,
            vm_entry.get_memory_option_flags(),
        );
        if let Err(e) = result {
            if let Err(e) = self.unassociate_address(virtual_address, size, pm_manager) {
                pr_err!("Failed to unmap memory: {:?}", e);
            }
            pr_err!("Failed to insert pages into vm_entry: {:?}", e);
        }
        self.lock.unlock();
        result
    }

    fn _update_page_table_with_vm_entry(
        &mut self,
        vm_entry: &mut VirtualMemoryEntry,
        pm_manager: &mut PhysicalMemoryManager,
        first_index: Option<MIndex>,
    ) -> Result<(), MemoryError> {
        assert!(self.lock.is_locked());
        let first_p_index = first_index.unwrap_or_else(|| vm_entry.get_memory_offset().to_index());
        let last_p_index = MSize::from_address(
            vm_entry.get_vm_start_address(),
            vm_entry.get_vm_end_address(),
        )
        .to_index();
        let vm_start_address = vm_entry.get_vm_start_address();
        let permission_flags = vm_entry.get_permission_flags();
        let option_flags = vm_entry.get_memory_option_flags();

        let local_object = vm_entry.get_object_mut();
        let _local_object_lock = local_object.lock.lock();

        let (shared_object, _shared_object_lock) = if let Some(s) = local_object.get_shared_object()
        {
            let l = s.lock.lock();
            (Some(s), Some(l))
        } else {
            (None, None)
        };

        for i in first_p_index..last_p_index {
            let physical_address;
            let permission;
            /* First, try to use local vm_page */
            if let Some(p) = local_object.get_vm_page_mut(i) {
                p.activate();
                physical_address = p.get_physical_address();
                permission = permission_flags;
            } else if let Some(s) = &shared_object
                && let Some(p) = s.get_vm_page(i)
            {
                if !p.is_activated() {
                    pr_warn!("vm_page({i}) is not activated");
                    continue;
                }
                physical_address = p.get_physical_address();
                // TODO: Check memory permission
                permission = permission_flags;
            } else {
                pr_warn!("vm_page(index: {i}) was not found");
                continue;
            }
            if let Err(e) = self.map_address_into_page_table(
                physical_address,
                vm_start_address + i.to_offset(),
                PAGE_SIZE,
                permission,
                option_flags,
                pm_manager,
            ) {
                pr_err!(
                    "Failed to update page table (target address: {}, index: {i}): {e:?}",
                    vm_start_address + i.to_offset()
                );
                for unassociate_i in first_p_index..i {
                    let address = vm_start_address + unassociate_i.to_offset();
                    if let Err(u_e) = self.unassociate_address(address, PAGE_SIZE, pm_manager) {
                        pr_err!("Failed to rollback paging");
                        return Err(u_e);
                    }
                    if let Some(p) = local_object.get_vm_page_mut(unassociate_i) {
                        p.inactivate();
                    }
                }
                return Err(e);
            }
        }
        Ok(())
    }

    /// Map the given physical address
    ///
    /// This function will search available virtual address if `virtual_address` is `None`.
    /// This does not flush page table cache.
    /// If map non-linearly, use [`Self::alloc_virtual_address`].
    pub fn map_address(
        &mut self,
        physical_address: PAddress,
        virtual_address: Option<VAddress>,
        size: MSize,
        permission: MemoryPermissionFlags,
        option: MemoryOptionFlags,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<VAddress, MemoryError> {
        Self::check_align(Some(physical_address), virtual_address, Some(size))?;
        self.lock.lock();
        let result = self._map_address(
            physical_address,
            virtual_address,
            size,
            permission,
            option,
            pm_manager,
        );
        self.lock.unlock();
        result
    }

    fn _map_address(
        &mut self,
        physical_address: PAddress,
        virtual_address: Option<VAddress>,
        size: MSize,
        permission: MemoryPermissionFlags,
        option: MemoryOptionFlags,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<VAddress, MemoryError> {
        assert!(self.lock.is_locked());
        /* Check the options */
        if option.is_stack() || option.is_heap() {
            if option.is_device_memory() || option.is_io_map() {
                return Err(MemoryError::MapAddressFailed);
            }
            if option.is_stack() == option.is_heap() {
                return Err(MemoryError::MapAddressFailed);
            }
        }
        let vm_entry = if let Some(vm_start_address) = virtual_address {
            /* assume virtual address is usable. */
            /*if !self.check_if_usable_address_range(address, address + size - 1) {
                return Err("Virtual Address is not usable.");
            }*/
            let vm_entry = self.alloc_vm_entry(option).inspect_err(|e| {
                pr_err!("Failed to allocate virtual memory entry: {e:?}");
            })?;
            init_struct!(
                *vm_entry,
                VirtualMemoryEntry::new(
                    vm_start_address,
                    size.to_end_address(vm_start_address),
                    permission,
                    option
                )
            );
            self.insert_vm_map_entry_into_list(vm_entry)
                .inspect_err(|e| {
                    get_kernel_manager_cluster()
                        .system_memory_manager
                        .free_vm_entry(vm_entry);
                    pr_err!("Failed to insert virtual memory entry: {e:?}");
                })?;
            vm_entry
        } else {
            self._alloc_virtual_address(size, permission, option)
                .inspect_err(|e| pr_err!("Failed to allocate address: {e:?}"))?
        };
        let address = vm_entry.get_vm_start_address();
        if let Err(e) = self.__map_address(physical_address, vm_entry, pm_manager) {
            unsafe { self.vm_entry.remove(&mut vm_entry.list) };
            get_kernel_manager_cluster()
                .system_memory_manager
                .free_vm_entry(vm_entry);
            Err(e)
        } else {
            Ok(address)
        }
    }

    fn __map_address(
        &mut self,
        physical_address: PAddress,
        vm_entry: &mut VirtualMemoryEntry,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        assert!(self.lock.is_locked());
        Self::check_align(Some(physical_address), None, None)?;
        let vm_start_address = vm_entry.get_vm_start_address();
        let size = vm_entry.get_size();
        let option = vm_entry.get_memory_option_flags();
        let permission = vm_entry.get_permission_flags();

        if !option.is_for_kernel() && !option.is_for_user() {
            vm_entry.set_memory_option_flags(option | MemoryOptionFlags::KERNEL);
        }
        if option.is_for_kernel() && permission.is_user_accessible() {
            pr_err!("Invalid Memory Permission");
            return Err(MemoryError::InternalError);
        }
        if option.is_io_map() && permission.is_executable() {
            pr_err!("Invalid Memory Permission");
            return Err(MemoryError::InternalError);
        }
        self.insert_pages_into_vm_entry(
            vm_entry,
            physical_address,
            vm_start_address,
            size,
            option,
        )?;

        if vm_entry.get_memory_option_flags().is_io_map() {
            vm_entry.set_memory_option_flags(
                vm_entry.get_memory_option_flags() | MemoryOptionFlags::ALLOW_HUGE,
            );
            if let Err(e) = self.map_address_into_page_table(
                physical_address,
                vm_start_address,
                size,
                permission,
                option,
                pm_manager,
            ) {
                pr_err!(
                    "Failed to map address(VirtualAddress: {}, PhysicalAddress: {}) with block_size: {:?}",
                    vm_start_address,
                    physical_address,
                    e
                );
                if let Err(e) = self.unassociate_address(vm_start_address, size, pm_manager) {
                    pr_err!(
                        "Failed to unmap address(VirtualAddress: {}): {:?}",
                        vm_start_address,
                        e
                    );
                }
                return Err(e);
            }
        } else {
            for i in MIndex::new(0)..size.to_index() {
                if let Err(e) = self.map_address_into_page_table(
                    physical_address + i.to_offset(),
                    vm_start_address + i.to_offset(),
                    PAGE_SIZE,
                    permission,
                    option,
                    pm_manager,
                ) {
                    pr_err!(
                        "Failed to map address(VirtualAddress: {}, PhysicalAddress: {}): {:?}",
                        vm_start_address + i.to_offset(),
                        physical_address + i.to_offset(),
                        e
                    );

                    for u_i in MIndex::new(0)..i {
                        if let Err(e) = self.unassociate_address(
                            vm_start_address + u_i.to_offset(),
                            PAGE_SIZE,
                            pm_manager,
                        ) {
                            pr_err!(
                                "Failed to unmap address(VirtualAddress: {}): {:?}",
                                vm_start_address + i.to_offset(),
                                e
                            );
                        }
                    }
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    pub fn free_address(
        &mut self,
        vm_start_address: VAddress,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        Self::check_align(None, Some(vm_start_address), None)?;
        if self.vm_entry.is_empty() {
            pr_err!("There is no entry.");
            return Err(MemoryError::InternalError);
        }
        self.lock.lock();
        if let Some(vm_entry) = find_vm_entry_mut!(self, vm_start_address) {
            let result = self._free_address(vm_entry, pm_manager);
            self.lock.unlock();
            result
        } else {
            self.lock.unlock();
            pr_err!("Cannot find vm_entry.");
            Err(MemoryError::InvalidAddress)
        }
    }

    pub(super) fn free_address_with_vm_entry(
        &mut self,
        vm_entry /* will be removed from list and freed */: &'static mut VirtualMemoryEntry,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        if self.vm_entry.is_empty() {
            pr_err!("There is no entry.");
            return Err(MemoryError::InternalError);
        }
        self.lock.lock();
        let result = self._free_address(vm_entry, pm_manager);
        self.lock.unlock();
        result
    }

    fn _free_address(
        &mut self,
        vm_entry /* will be removed from the list and freed */: &mut VirtualMemoryEntry,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        assert!(self.lock.is_locked());
        let first_p_index = vm_entry.get_memory_offset().to_index();
        let last_p_index = MSize::from_address(
            vm_entry.get_vm_start_address(),
            vm_entry.get_vm_end_address(),
        )
        .to_index();

        if vm_entry.get_memory_option_flags().is_io_map() {
            if let Err(e) = self.unassociate_address(
                vm_entry.get_vm_start_address(),
                vm_entry.get_size(),
                pm_manager,
            ) {
                pr_err!(
                    "Failed to unmap address({} ~ {}): {:?}",
                    vm_entry.get_vm_start_address(),
                    vm_entry.get_vm_end_address(),
                    e
                );
                return Err(e);
            }
            if !vm_entry
                .get_memory_option_flags()
                .should_not_free_phy_address()
            {
                let vm_object = vm_entry.get_object_mut();

                let _object_lock = vm_object.lock.lock();
                for i in first_p_index..=last_p_index {
                    if let Some(p) = vm_object.remove_vm_page(i) {
                        bug_on_err!(pm_manager.free(p.get_physical_address(), PAGE_SIZE, false));
                        get_kernel_manager_cluster()
                            .system_memory_manager
                            .free_vm_page(p, p.get_physical_address());
                    }
                }
            }
        } else {
            let mut processed = false;
            let vm_object = vm_entry.get_object_mut();
            if let Some(shared_object) = {
                let _vm_object_lock = vm_object.lock.lock();
                vm_entry.get_object().get_shared_object()
            } {
                let _shared_object_lock = shared_object.lock.lock();
                if shared_object.get_reference_count() > 1 {
                    for i in first_p_index..=last_p_index {
                        if shared_object.get_vm_page(i).is_some()
                            && let Err(e) = self.unassociate_address(
                                vm_entry.get_vm_start_address() + i.to_offset(),
                                PAGE_SIZE,
                                pm_manager,
                            )
                        {
                            pr_err!(
                                "Failed to unmap address({}): {:?}",
                                vm_entry.get_vm_start_address() + i.to_offset(),
                                e
                            );
                            return Err(e);
                        }
                    }
                    processed = true;
                } else {
                    drop(_shared_object_lock);
                    assert!(self.try_to_unshadow_vm_object(vm_entry));
                    /* vm_pages will be removed at the below */
                }
            }
            if !processed {
                let start_address = vm_entry.get_vm_start_address();
                let options = vm_entry.get_memory_option_flags();
                let vm_object = vm_entry.get_object_mut();
                let _vm_object_lock = vm_object.lock.lock();
                for i in first_p_index..=last_p_index {
                    if let Some(p) = vm_object.remove_vm_page(i) {
                        if let Err(e) = self.unassociate_address(
                            start_address + i.to_offset(),
                            PAGE_SIZE,
                            pm_manager,
                        ) {
                            pr_err!(
                                "Failed to unmap address({}): {:?}",
                                vm_entry.get_vm_start_address() + i.to_offset(),
                                e
                            );
                            return Err(e);
                        }
                        if !options.should_not_free_phy_address()
                            && let Err(e) =
                                pm_manager.free(p.get_physical_address(), PAGE_SIZE, false)
                        {
                            pr_err!("Failed to free physical memory: {:?}", e);
                        }
                        get_kernel_manager_cluster()
                            .system_memory_manager
                            .free_vm_page(p, p.get_physical_address());
                    }
                }
            }
        }
        self._update_paging(vm_entry.get_vm_start_address(), vm_entry.get_size());
        unsafe { self.vm_entry.remove(&mut vm_entry.list) };
        self.adjust_vm_entries();
        /* destory vm_object and vm_entry */
        {
            let vm_object = vm_entry.get_object_mut();
            let _vm_object_lock = vm_object.lock.lock();
            vm_entry.set_disabled();
        }
        /* do not free vm_object which is not allocated */
        get_kernel_manager_cluster()
            .system_memory_manager
            .free_vm_entry(vm_entry);

        Ok(())
    }

    /// Allocate VirtualMemoryEntry from the pool
    ///
    /// Allocating entry may cause nested memory allocations for the pools when they short.
    /// Then, the memory map may be changed. This function does not care about it.
    fn alloc_vm_entry(
        &mut self,
        option: MemoryOptionFlags,
    ) -> Result<&'static mut VirtualMemoryEntry, MemoryError> {
        assert!(self.lock.is_locked());
        loop {
            match get_kernel_manager_cluster()
                .system_memory_manager
                .alloc_vm_entry(self.is_kernel_virtual_memory_manager(), option)
            {
                Ok(e) => return Ok(e),
                Err(MemoryError::EntryPoolRunOut) => {
                    if option.is_no_wait() {
                        return Err(MemoryError::EntryPoolRunOut);
                    } else {
                        if self.is_kernel_virtual_memory_manager() {
                            self.lock.unlock();
                        }
                        SystemMemoryManager::pool_alloc_worker(
                            SystemMemoryManager::ALLOC_VM_ENTRY_FLAG,
                        );
                        if self.is_kernel_virtual_memory_manager() {
                            self.lock.lock();
                        }
                        continue;
                    }
                }
                Err(e) => return Err(e),
            };
        }
    }

    fn insert_vm_map_entry_into_list(
        &mut self,
        vm_entry: &mut VirtualMemoryEntry,
    ) -> Result<(), MemoryError> {
        if self.vm_entry.is_empty() {
            unsafe { self.vm_entry.insert_head(&mut vm_entry.list) };
        } else if let Some(prev_entry) =
            find_previous_vm_entry_mut!(self, vm_entry.get_vm_start_address())
        {
            unsafe {
                self.vm_entry
                    .insert_after(&mut prev_entry.list, &mut vm_entry.list)
            };
        } else if vm_entry.get_vm_end_address()
            < self
                .vm_entry
                .get_first_entry(offset_of!(VirtualMemoryEntry, list))
                .map(|e| unsafe { &*e })
                .unwrap()
                .get_vm_start_address()
        {
            unsafe { self.vm_entry.insert_head(&mut vm_entry.list) };
        } else {
            pr_err!("Cannot insert Virtual Memory Entry.");
            return Err(MemoryError::InternalError);
        }
        self.adjust_vm_entries();
        Ok(())
    }

    /// Insert pages into VirtualMemoryEntry without applying PageManager.
    ///
    /// This function allocates vm_page and inserts it into vm_entry.
    /// virtual_address must be allocated.
    fn insert_pages_into_vm_entry(
        &mut self,
        vm_entry: &mut VirtualMemoryEntry,
        physical_address: PAddress,
        virtual_address: VAddress,
        size: MSize,
        option: MemoryOptionFlags,
    ) -> Result<(), MemoryError> {
        assert_eq!(physical_address & !PAGE_MASK, 0);
        assert_eq!(virtual_address & !PAGE_MASK, 0);
        assert_eq!(size & !PAGE_MASK, 0);
        assert!(!size.is_zero());
        assert!(self.lock.is_locked());
        let start_address = vm_entry.get_vm_start_address();
        let memory_offset_index = vm_entry.get_memory_offset().to_index();
        let memory_options = vm_entry.get_memory_option_flags();
        let vm_object = vm_entry.get_object_mut();
        let _vm_object_lock = vm_object.lock.lock();

        for i in MIndex::new(0)..size.to_index() {
            let current_virtual_address = virtual_address + i.to_offset();
            let current_physical_address = physical_address + i.to_offset();
            let p_index =
                (current_virtual_address - start_address).to_index() + memory_offset_index;
            let vm_page;
            loop {
                match get_kernel_manager_cluster()
                    .system_memory_manager
                    .alloc_vm_page(
                        current_physical_address,
                        self.is_kernel_virtual_memory_manager(),
                        option,
                    ) {
                    Ok(e) => {
                        vm_page = e;
                        break;
                    }
                    Err(MemoryError::EntryPoolRunOut) => {
                        if option.is_no_wait() {
                            return Err(MemoryError::EntryPoolRunOut);
                        } else {
                            if self.is_kernel_virtual_memory_manager() {
                                self.lock.unlock();
                            }
                            SystemMemoryManager::pool_alloc_worker(
                                SystemMemoryManager::ALLOC_VM_PAGE_FLAG,
                            );
                            if self.is_kernel_virtual_memory_manager() {
                                self.lock.lock();
                            }
                            continue;
                        }
                    }
                    Err(e) => {
                        return Err(e);
                    }
                }
            }
            init_struct!(
                *vm_page,
                VirtualMemoryPage::new(current_physical_address, p_index)
            );
            vm_page.set_page_status(memory_options);
            vm_page.activate();
            vm_object.add_vm_page(p_index, vm_page);
        }
        Ok(())
    }

    fn try_to_unshadow_vm_object(&mut self, vm_entry: &mut VirtualMemoryEntry) -> bool {
        assert!(self.lock.is_locked());
        let vm_object = vm_entry.get_object_mut();
        let _lock = vm_object.lock.lock();
        if !vm_object.has_shadow_entry() {
            return true;
        }
        let target_vm_object = vm_object.get_shared_object().unwrap();

        let _target_lock = target_vm_object.lock.lock();
        assert_ne!(target_vm_object.get_reference_count(), 0);
        if target_vm_object.get_reference_count() > 1 {
            return false;
        }
        vm_object.unset_shared_object(target_vm_object);
        assert_eq!(target_vm_object.get_reference_count(), 0);
        assert!(!target_vm_object.has_shadow_entry());
        if let Some(p) = target_vm_object.get_io_map_base_address() {
            assert!(!vm_object.has_vm_page());
            vm_object.set_io_map_base_address(p);
        } else if target_vm_object.has_vm_page() {
            while let Some(p) = target_vm_object.take_vm_page() {
                vm_object.add_vm_page(p.get_p_index(), p);
            }
        }
        /* `target_vm_object` must be allocated one */
        get_kernel_manager_cluster()
            .system_memory_manager
            .free_vm_object(target_vm_object);
        true
    }

    pub(super) fn share_memory_with_user(
        &mut self,
        user_vm_manager: &mut Self,
        kernel_virtual_address: VAddress,
        user_virtual_address: VAddress,
        user_permission: MemoryPermissionFlags,
        user_option: MemoryOptionFlags,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        Self::check_align(None, Some(kernel_virtual_address), None)?;
        Self::check_align(None, Some(user_virtual_address), None)?;

        loop {
            user_vm_manager.lock.lock();
            if self.lock.try_lock().is_ok() {
                break;
            }
            user_vm_manager.lock.unlock();
        }
        let Some(kernel_vm_entry) = find_vm_entry_mut!(self, kernel_virtual_address) else {
            pr_err!("{} is not found.", kernel_virtual_address);
            self.lock.unlock();
            user_vm_manager.lock.unlock();
            return Err(MemoryError::InvalidAddress);
        };
        let user_virtual_end_address = kernel_vm_entry
            .get_size()
            .to_end_address(user_virtual_address);

        let original_vm_object = kernel_vm_entry.get_object_mut();
        let _original_vm_object_lock = original_vm_object.lock.lock();
        if original_vm_object.has_shadow_entry() {
            pr_err!("Nested shared memory is not supported.");
            self.lock.unlock();
            user_vm_manager.lock.unlock();
            return Err(MemoryError::InternalError);
        }

        /* Allocate vm_object to share */
        let vm_object;
        loop {
            vm_object = match get_kernel_manager_cluster()
                .system_memory_manager
                .alloc_vm_object(self.is_kernel_virtual_memory_manager(), user_option)
            {
                Ok(e) => Ok(e),
                Err(MemoryError::EntryPoolRunOut) => {
                    self.lock.unlock();
                    SystemMemoryManager::pool_alloc_worker(
                        SystemMemoryManager::ALLOC_VM_ENTRY_FLAG,
                    );
                    self.lock.lock();
                    continue;
                }

                Err(e) => Err(e),
            };
            break;
        }
        let Ok(shared_vm_object) = vm_object else {
            self.lock.unlock();
            user_vm_manager.lock.unlock();
            return vm_object.map(|_| ());
        };

        /* Assume user_virtual_address is usable. */
        let user_vm_map_entry = user_vm_manager
            .alloc_vm_entry(user_option)
            .inspect_err(|e| {
                get_kernel_manager_cluster()
                    .system_memory_manager
                    .free_vm_object(shared_vm_object);
                self.lock.unlock();
                user_vm_manager.lock.unlock();
                pr_err!("Failed to allocate virtual memory entry: {e:?}");
            })?;
        init_struct!(
            *user_vm_map_entry,
            VirtualMemoryEntry::new(
                user_virtual_address,
                user_virtual_end_address,
                user_permission,
                user_option,
            )
        );
        user_vm_manager
            .insert_vm_map_entry_into_list(user_vm_map_entry)
            .inspect_err(|e| {
                get_kernel_manager_cluster()
                    .system_memory_manager
                    .free_vm_entry(user_vm_map_entry);
                get_kernel_manager_cluster()
                    .system_memory_manager
                    .free_vm_object(shared_vm_object);
                self.lock.unlock();
                user_vm_manager.lock.unlock();
                pr_err!("Failed to insert virtual memory entry: {e:?}");
            })?;

        init_struct!(*shared_vm_object, VirtualMemoryObject::new());
        drop(_original_vm_object_lock);
        core::mem::swap(shared_vm_object, original_vm_object);

        {
            let _shared_vm_object_lock = shared_vm_object.lock.lock();
            let _original_vm_object_lock = original_vm_object.lock.lock();
            let user_vm_object = user_vm_map_entry.get_object_mut();
            let _user_vm_object_lock = user_vm_object.lock.lock();

            original_vm_object.set_shared_object(shared_vm_object);
            user_vm_object.set_shared_object(shared_vm_object);
        }

        self.lock.unlock();
        if let Err(e) =
            user_vm_manager._update_page_table_with_vm_entry(user_vm_map_entry, pm_manager, None)
        {
            {
                let _shared_object_lock = shared_vm_object.lock.lock();
                let user_vm_object = user_vm_map_entry.get_object_mut();
                let _user_vm_object_lock = user_vm_object.lock.lock();
                user_vm_object.unset_shared_object(shared_vm_object);
            }
            unsafe { user_vm_manager.vm_entry.remove(&mut user_vm_map_entry.list) };
            user_vm_manager.adjust_vm_entries();
            user_vm_map_entry.set_disabled();
            get_kernel_manager_cluster()
                .system_memory_manager
                .free_vm_entry(user_vm_map_entry);

            /* do not free `shared_vm_object` at this time */
            user_vm_manager.lock.unlock();
            return Err(e);
        }
        user_vm_manager.lock.unlock();
        Ok(())
    }

    fn map_address_into_page_table(
        &mut self,
        physical_address: PAddress,
        virtual_address: VAddress,
        size: MSize,
        permission: MemoryPermissionFlags,
        option: MemoryOptionFlags,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        assert!(self.lock.is_locked());
        self.page_manager
            .associate_address(
                pm_manager,
                physical_address,
                virtual_address,
                size,
                permission,
                option,
            )
            .map_err(|e| MemoryError::PagingError(e))
    }

    fn unassociate_address(
        &self,
        virtual_address: VAddress,
        size: MSize,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        assert!(self.lock.is_locked());
        self.page_manager
            .unassociate_address(virtual_address, size, pm_manager)
            .map_err(|e| MemoryError::PagingError(e))
    }

    fn try_expand_vm_entry(
        &mut self,
        vm_entry: &mut VirtualMemoryEntry,
        new_size: MSize,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> bool {
        assert!(self.lock.is_locked());
        let old_size = vm_entry.get_size();
        if old_size >= new_size {
            return true;
        }
        let is_stack = vm_entry.get_memory_option_flags().is_stack();
        /* Check if the new address range is overlapped with prev/next entries */
        let new_end_address = if is_stack {
            vm_entry.get_vm_end_address()
        } else {
            new_size.to_end_address(vm_entry.get_vm_start_address())
        };
        let new_start_address = if is_stack {
            vm_entry.get_vm_start_address() - (new_size - old_size)
        } else {
            vm_entry.get_vm_start_address()
        };
        const OFFSET: usize = offset_of!(VirtualMemoryEntry, list);
        if let Some(next_entry) = vm_entry.list.get_next(OFFSET).map(|e| unsafe { &*e })
            && new_end_address >= next_entry.get_vm_start_address()
        {
            return false;
        } else if is_stack
            && let Some(prev_entry) = vm_entry.list.get_prev(OFFSET).map(|e| unsafe { &*e })
            && new_start_address <= prev_entry.get_vm_end_address()
        {
            return false;
        } else if new_end_address >= MAX_VIRTUAL_ADDRESS {
            return false;
        }

        /* Setup vm_page */
        let old_last_p_index = old_size.to_index() - MIndex::new(1);
        if is_stack {
            unimplemented!()
        } else if vm_entry.get_memory_option_flags().is_heap() {
            let not_associated_virtual_address = vm_entry.get_vm_end_address() + MSize::new(1);
            vm_entry.set_vm_end_address(new_end_address);
            /* TODO: Lazy mapping */
            /* To make the process simple, allocating continuous physical address,
            this should be fixed at the lazy mapping */
            let allocation_size = new_size - old_size;
            match MemoryManager::allocate_physical_memory(
                allocation_size,
                MOrder::new(PAGE_SHIFT),
                pm_manager,
            ) {
                Ok(a) => {
                    if let Err(e) = self.insert_pages_into_vm_entry(
                        vm_entry,
                        a,
                        not_associated_virtual_address,
                        allocation_size,
                        vm_entry.get_memory_option_flags(),
                    ) {
                        pr_err!("Failed to map expanded area: {:?}", e);
                        bug_on_err!(pm_manager.free(a, allocation_size, false));
                        return false;
                    }
                }
                Err(e) => {
                    pr_err!("Failed to allocate memory: {e:?}");
                    return false;
                }
            }
        } else if vm_entry.get_memory_option_flags().is_io_map() {
            let not_associated_virtual_address = vm_entry.get_vm_end_address() + MSize::new(1);
            vm_entry.set_vm_end_address(new_end_address);
            let vm_object = vm_entry.get_object();
            let _vm_object_lock = vm_object.lock.lock();
            let not_associated_physical_address = vm_object
                .get_vm_page(old_last_p_index)
                .unwrap()
                .get_physical_address()
                + PAGE_SIZE;
            drop(_vm_object_lock);

            if let Err(e) = self.insert_pages_into_vm_entry(
                vm_entry,
                not_associated_physical_address,
                not_associated_virtual_address,
                new_size - old_size,
                vm_entry.get_memory_option_flags(),
            ) {
                pr_err!("Failed to map expanded area: {:?}", e);
                return false;
            }
        } else {
            return false;
        }
        if let Err(e) =
            self._update_page_table_with_vm_entry(vm_entry, pm_manager, Some(old_last_p_index))
        {
            pr_err!("Failed to update paging table to expanded area: {:?}", e);
            return false;
        }
        true
    }

    pub fn resize_memory_mapping(
        &mut self,
        virtual_address: VAddress,
        new_size: MSize,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<VAddress, MemoryError> {
        Self::check_align(None, Some(virtual_address), Some(new_size))?;
        if new_size.is_zero() {
            pr_err!("Size is zero.");
            return Err(MemoryError::InvalidSize);
        } else if self.vm_entry.is_empty() {
            pr_err!("There is no entry.");
            return Err(MemoryError::InvalidAddress); /* Is it ok? */
        }
        self.lock.lock();
        if let Some(vm_entry) = find_vm_entry_mut!(self, virtual_address) {
            let option = vm_entry.get_memory_option_flags();
            if !option.is_io_map() && !option.is_stack() && !option.is_heap() {
                self.lock.unlock();
                pr_err!("Expected the address is for io_map, stack, or heap");
                return Err(MemoryError::InvalidAddress);
            }
            if self.try_expand_vm_entry(vm_entry, new_size, pm_manager) {
                self.lock.unlock();
                return Ok(virtual_address);
            } else if option.is_heap() || option.is_stack() {
                return Err(MemoryError::AddressNotAvailable);
            }
            assert!(option.is_io_map());
            let vm_object = vm_entry.get_object();
            let _vm_object_lock = vm_object.lock.lock();
            assert!(!vm_object.has_shadow_entry(), "unimplemented");
            let permission = vm_entry.get_permission_flags();
            let physical_address = vm_object
                .get_vm_page(vm_entry.get_memory_offset().to_index())
                .unwrap()
                .get_physical_address();
            /* Assume: p_index is the first of mapping address */
            vm_entry
                .set_memory_option_flags(option | MemoryOptionFlags::DO_NOT_FREE_PHYSICAL_ADDRESS);
            drop(_vm_object_lock);

            /* Free the old address and map again */
            if let Err(e) = self._free_address(vm_entry, pm_manager) {
                self.lock.unlock();
                pr_err!("Failed to free memory to remap: {:?}", e);
                return Err(e);
            }
            let result = self._map_address(
                physical_address,
                None,
                new_size,
                permission,
                option,
                pm_manager,
            );
            self.lock.unlock();
            result
        } else {
            self.lock.unlock();
            Err(MemoryError::InvalidAddress)
        }
    }

    pub fn free_all_mapping(
        &mut self,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        self.lock.lock();
        while let Some(e) = self
            .vm_entry
            .get_last_entry_mut(offset_of!(VirtualMemoryEntry, list))
            .map(|e| unsafe { &mut *e })
        {
            if let Err(e) = self._free_address(e, pm_manager) {
                /* TODO: recovery */
                self.lock.unlock();
                return Err(e);
            }
        }
        if let Err(e) = self.page_manager.destroy_page_table(pm_manager) {
            self.lock.unlock();
            pr_err!("Failed to free page table: {:?}", e);
            return Err(MemoryError::PagingError(e));
        }
        self._update_paging_all();
        self.lock.unlock();
        Ok(())
    }

    pub fn get_physical_address_list(
        &self,
        virtual_address: VAddress,
        offset: MIndex,
        mut number_of_pages: MIndex,
        list_buffer: &mut [PAddress],
    ) -> Result<usize, MemoryError> {
        Self::check_align(None, Some(virtual_address), None)?;
        if number_of_pages.to_usize() > list_buffer.len() {
            number_of_pages = MIndex::new(list_buffer.len());
        }
        self.lock.lock();
        if let Some(vm_entry) = find_vm_entry!(self, virtual_address) {
            /* TODO: check permissions */
            let mut n = 0;
            let vm_object = vm_entry.get_object();
            let _vm_object_lock = vm_object.lock.lock();
            for index in offset..(offset + number_of_pages) {
                if let Some(p) = vm_object.get_vm_page(index) {
                    list_buffer[n] = p.get_physical_address();
                    n += 1;
                } else {
                    break;
                }
            }
            self.lock.unlock();
            Ok(n)
        } else {
            self.lock.unlock();
            pr_err!("Entry is not found.");
            Err(MemoryError::InvalidAddress)
        }
    }

    fn adjust_vm_entries(&mut self) {
        /* Currently, do nothing */
    }

    fn is_overlapped<T: Address>(range_1: &RangeInclusive<T>, range_2: &RangeInclusive<T>) -> bool {
        range_1.contains(range_2.start())
            || range_1.contains(range_2.end())
            || range_2.contains(range_1.start())
            || range_2.contains(range_1.end())
    }

    pub fn dump_memory_manager(
        &self,
        start_vm_address: Option<VAddress>,
        end_vm_address: Option<VAddress>,
    ) {
        let start = start_vm_address.unwrap_or(VAddress::new(0));
        let end = end_vm_address.unwrap_or(MAX_VIRTUAL_ADDRESS);
        kprintln!(
            "Is kernel virtual memory manager:{}",
            self.is_kernel_virtual_memory_manager()
        );
        if self.vm_entry.is_empty() {
            kprintln!("There is no root entry.");
            return;
        }
        let offset = offset_of!(VirtualMemoryEntry, list);
        self.lock.lock();
        let mut entry = self
            .vm_entry
            .get_first_entry(offset)
            .map(|e| unsafe { &*e })
            .unwrap();
        loop {
            if entry.get_vm_start_address() < start || entry.get_vm_end_address() > end {
                let next = entry.list.get_next(offset).map(|e| unsafe { &*e });
                if next.is_none() {
                    break;
                }
                entry = next.unwrap();
                continue;
            }
            let has_shadow = {
                let vm_object = entry.get_object();
                match vm_object.lock.try_lock() {
                    Ok(_lock) => vm_object.has_shadow_entry(),
                    Err(_) => false,
                }
            };
            kprintln!(
                "Virtual Address: {:>#18X}, Size: {:>#18X}, W: {:>5}, U: {:>5}, E: {:>5}, Shared: {:>5}",
                entry.get_vm_start_address().to_usize(),
                MSize::from_address(entry.get_vm_start_address(), entry.get_vm_end_address())
                    .to_usize(),
                entry.get_permission_flags().is_writable(),
                entry.get_permission_flags().is_user_accessible(),
                entry.get_permission_flags().is_executable(),
                has_shadow
            );
            let first_p_index = entry.get_memory_offset().to_index();
            let last_p_index = MIndex::from_offset(
                entry.get_vm_end_address() - entry.get_vm_start_address()
                    + entry.get_memory_offset(), /* Is it ok? */
            ) + MIndex::new(1);

            let mut omit_info = (
                false,            /* omitted */
                false,            /* is last not found */
                PAddress::new(0), /* last address*/
                false,            /* is last shared */
            );

            let vm_object = entry.get_object();
            let Ok(_vm_object_lock) = vm_object.lock.try_lock() else {
                pr_warn!("Failed to lock object");
                let next = entry.list.get_next(offset).map(|e| unsafe { &*e });
                if next.is_none() {
                    break;
                }
                entry = next.unwrap();
                continue;
            };
            let (shared_object, _shared_object_lock) = if has_shadow {
                let shared_object = vm_object.get_shared_object().unwrap();
                let _shared_object_lock = shared_object.lock.lock();
                (Some(shared_object), Some(_shared_object_lock))
            } else {
                (None, None)
            };
            for i in first_p_index..last_p_index {
                let (vm_page, is_shared) = vm_object
                    .get_vm_page(i)
                    .map(|p| (Some(p), false))
                    .or_else(|| {
                        shared_object
                            .as_ref()
                            .and_then(|s| s.get_vm_page(i).map(|p| (Some(p), true)))
                    })
                    .unwrap_or((None, false));

                if let Some(p) = vm_page {
                    if omit_info.1 {
                        kprintln!("...\n - {:>6} Not Found", i.to_usize() - 1);
                        omit_info.1 = false;
                    } else if omit_info.2 + PAGE_SIZE == p.get_physical_address()
                        && omit_info.3 == is_shared
                    {
                        omit_info.0 = true;
                        omit_info.2 += PAGE_SIZE;
                        continue;
                    } else if omit_info.0 {
                        kprintln!(
                            "...\n - {:>6} Physical Address: {:>#18X}{}",
                            i.to_usize() - 1,
                            omit_info.2.to_usize(),
                            if has_shadow {
                                if is_shared { " (Shared)" } else { " (Local)" }
                            } else {
                                ""
                            }
                        );
                        omit_info.0 = false;
                    }
                    kprintln!(
                        " - {:>6} Physical Address: {:>#18X}{}",
                        i.to_usize(),
                        p.get_physical_address().to_usize(),
                        if has_shadow {
                            if is_shared { " (Shared)" } else { " (Local)" }
                        } else {
                            ""
                        }
                    );
                    omit_info.2 = p.get_physical_address();
                    omit_info.3 = is_shared;
                } else if !omit_info.1 {
                    kprintln!(" - {:>6} Not Found", i.to_usize());
                    omit_info.1 = true;
                    omit_info.2 = PAddress::new(0);
                    omit_info.3 = false;
                }
            }
            if omit_info.1 {
                kprintln!("...\n - {:>6} Not Found (Fin)", last_p_index.to_usize() - 1);
            } else if omit_info.0 {
                kprintln!(
                    "...\n - {:>6} Physical Address: {:>#18X} (Fin)",
                    last_p_index.to_usize() - 1,
                    omit_info.2.to_usize()
                );
            }
            let next = entry.list.get_next(offset).map(|e| unsafe { &*e });
            if next.is_none() {
                break;
            }
            kprintln!(""); // \n
            entry = next.unwrap();
        }
        kprintln!("----Page Manager----");
        self.page_manager
            .dump_table(start_vm_address, end_vm_address);
        self.lock.unlock();
    }
}
