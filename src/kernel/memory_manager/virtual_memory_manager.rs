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
    MemoryError,
    data_type::{
        Address, MIndex, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress, VAddress,
    },
    physical_memory_manager::PhysicalMemoryManager,
    system_memory_manager::SystemMemoryManager,
};

use crate::arch::target_arch::context::memory_layout::{
    MALLOC_END_ADDRESS, MALLOC_START_ADDRESS, MAP_END_ADDRESS, MAP_START_ADDRESS,
    USER_STACK_END_ADDRESS, USER_STACK_START_ADDRESS, get_direct_map_base_address,
    get_direct_map_size, get_direct_map_start_address,
};
use crate::arch::target_arch::paging::{
    MAX_VIRTUAL_ADDRESS, PAGE_MASK, PAGE_SIZE, PAGE_SIZE_USIZE, PageManager,
};

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
        pr_debug!(
            "Direct map: VA [{:#016X} ~ {:#016X}] => PA: [{:#016X} ~ {:#016X}] (Size: {:#X})",
            start_virtual_address.to_usize(),
            map_size.to_end_address(start_virtual_address).to_usize(),
            start_physical_address.to_usize(),
            map_size.to_end_address(start_physical_address).to_usize(),
            map_size.to_usize()
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
        PageManager::update_page_cache(address, range);
    }

    pub fn update_paging(&self, address: VAddress, range: MSize) {
        self.lock.lock();
        self._update_paging(address, range);
        self.lock.unlock();
    }

    fn _update_paging_all(&self) {
        PageManager::update_page_cache_all();
    }

    /// Allocate the virtual address and map the given physical address
    ///
    /// This function will search available virtual address
    /// to map physical_address ~ physical_address + size, and then map physical_address linearly.
    /// This does not flush page table cache.
    /// If map non-linearly, use [`alloc_virtual_address`].
    pub fn alloc_and_map_virtual_address(
        &mut self,
        size: MSize,
        physical_address: PAddress,
        permission: MemoryPermissionFlags,
        option: MemoryOptionFlags,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<VAddress, MemoryError> {
        Self::check_align(Some(physical_address), None, Some(size))?;
        self.lock.lock();
        let vm_start_address = if let Some(address) = self.find_usable_memory_area(size, option) {
            address
        } else {
            self.lock.unlock();
            pr_warn!("Virtual Address is not available.");
            return Err(MemoryError::AddressNotAvailable);
        };
        let result = self._map_address(
            physical_address,
            Some(vm_start_address),
            size,
            permission,
            option,
            pm_manager,
        );
        self.lock.unlock();
        result
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
        let entry = if let Some(address) = self.find_usable_memory_area(size, option) {
            VirtualMemoryEntry::new(address, size.to_end_address(address), permission, option)
        } else {
            self.lock.unlock();
            pr_warn!("Virtual Address is not available.");
            return Err(MemoryError::AddressNotAvailable);
        };

        let result = self.insert_vm_map_entry_into_list(entry, option);
        self.lock.unlock();
        result
    }

    /// Map virtual_address to physical_address with size.
    ///
    /// This function maps virtual_address to physical_address into vm_entry.
    /// vm_entry must be inserted in [`Self::vm_entry`]. (use the entry from [`alloc_virtual_address`])
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
        .to_index(); /* OK? */
        let vm_start_address = vm_entry.get_vm_start_address();
        let permission_flags = vm_entry.get_permission_flags();
        let option_flags = vm_entry.get_memory_option_flags();

        let (target_object, is_shared_object) =
            if let Some(s) = vm_entry.get_object().get_shared_object() {
                (s, true)
            } else {
                (vm_entry.get_object_mut(), false)
            };
        let _lock = target_object.lock.lock();

        for i in first_p_index..=last_p_index {
            if let Some(p) = target_object.get_vm_page_mut(i) {
                if !is_shared_object {
                    p.activate();
                }
                if let Err(e) = self.map_address_into_page_table(
                    p.get_physical_address(),
                    vm_start_address + i.to_offset(),
                    PAGE_SIZE,
                    permission_flags,
                    option_flags,
                    pm_manager,
                ) {
                    pr_err!(
                        "Failed to update paging(Address: {}, Index: {}): {:?}",
                        vm_start_address + i.to_offset(),
                        i,
                        e
                    );
                    for unassociate_i in first_p_index..i {
                        let address = vm_start_address + unassociate_i.to_offset();
                        if let Err(u_e) = self.unassociate_address(address, PAGE_SIZE, pm_manager) {
                            pr_err!("Failed to rollback paging(VirtualAddress: {}).", address);
                            return Err(u_e);
                        }
                        if !is_shared_object
                            && let Some(p) = target_object.get_vm_page_mut(unassociate_i)
                        {
                            p.inactivate()
                        }
                    }
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    pub fn map_address(
        &mut self,
        physical_address: PAddress,
        virtual_address: Option<VAddress>,
        size: MSize,
        permission: MemoryPermissionFlags,
        option: MemoryOptionFlags,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<VAddress, MemoryError> {
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
        mut option: MemoryOptionFlags,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<VAddress, MemoryError> {
        assert!(self.lock.is_locked());
        Self::check_align(Some(physical_address), virtual_address, Some(size))?;

        if !option.is_for_kernel() && !option.is_for_user() {
            option = option | MemoryOptionFlags::KERNEL;
        }
        if option.is_for_kernel() && permission.is_user_accessible() {
            pr_err!("Invalid Memory Permission");
            return Err(MemoryError::InternalError);
        }
        if option.is_io_map() && permission.is_executable() {
            pr_err!("Invalid Memory Permission");
            return Err(MemoryError::InternalError);
        }

        let mut vm_entry = if let Some(vm_start_address) = virtual_address {
            /* assume virtual address is usable. */
            /*if !self.check_if_usable_address_range(address, address + size - 1) {
                return Err("Virtual Address is not usable.");
            }*/
            VirtualMemoryEntry::new(
                vm_start_address,
                size.to_end_address(vm_start_address),
                permission,
                option,
            )
        } else if let Some(vm_start_address) = self.find_usable_memory_area(size, option) {
            VirtualMemoryEntry::new(
                vm_start_address,
                size.to_end_address(vm_start_address),
                permission,
                option,
            )
        } else {
            pr_err!("Virtual Address is not available.");
            return Err(MemoryError::AddressNotAvailable);
        };

        let vm_start_address = vm_entry.get_vm_start_address();
        self.insert_pages_into_vm_entry(
            &mut vm_entry,
            physical_address,
            vm_start_address,
            size,
            option,
        )?;

        let vm_entry = self.insert_vm_map_entry_into_list(vm_entry, option)?;

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
                unsafe { self.vm_entry.remove(&mut vm_entry.list) };
                get_kernel_manager_cluster()
                    .system_memory_manager
                    .free_vm_entry(vm_entry);
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
                    unsafe { self.vm_entry.remove(&mut vm_entry.list) };
                    get_kernel_manager_cluster()
                        .system_memory_manager
                        .free_vm_entry(vm_entry);
                    return Err(e);
                }
            }
        }
        Ok(vm_start_address)
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
            assert!(!vm_entry.get_object().is_shadow_entry());
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
                for i in first_p_index..=last_p_index {
                    if let Some(p) = vm_entry.get_object_mut().remove_vm_page(i) {
                        if let Err(e) = pm_manager.free(p.get_physical_address(), PAGE_SIZE, false)
                        {
                            pr_err!("Failed to free physical memory: {:?}", e);
                        }
                        get_kernel_manager_cluster()
                            .system_memory_manager
                            .free_vm_page(p, p.get_physical_address());
                    }
                }
            }
        } else {
            let mut processed = false;
            if let Some(shared_object) = vm_entry.get_object().get_shared_object() {
                let _lock = shared_object.lock.lock();
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
                    vm_entry.get_object_mut().unset_shared_object(shared_object);
                    drop(_lock);
                    core::mem::swap(shared_object, vm_entry.get_object_mut());
                    /* delete shared object(Invalid) */
                    shared_object.set_disabled();
                    get_kernel_manager_cluster()
                        .system_memory_manager
                        .free_vm_object(shared_object);
                }
            }
            if !processed {
                for i in first_p_index..=last_p_index {
                    if let Some(p) = vm_entry.get_object_mut().remove_vm_page(i) {
                        if let Err(e) = self.unassociate_address(
                            vm_entry.get_vm_start_address() + i.to_offset(),
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
                        if !vm_entry
                            .get_memory_option_flags()
                            .should_not_free_phy_address()
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
        vm_entry.set_disabled();
        get_kernel_manager_cluster()
            .system_memory_manager
            .free_vm_entry(vm_entry);

        Ok(())
    }

    /// Allocate VirtualMemoryEntry from the pool and chain it into [`Self::vm_entry`]
    fn insert_vm_map_entry_into_list(
        &mut self,
        source: VirtualMemoryEntry,
        option: MemoryOptionFlags,
    ) -> Result<&'static mut VirtualMemoryEntry, MemoryError> {
        assert!(self.lock.is_locked());
        let vm_entry;
        loop {
            vm_entry = match get_kernel_manager_cluster()
                .system_memory_manager
                .alloc_vm_entry(self.is_kernel_virtual_memory_manager(), option)
            {
                Ok(e) => Ok(e),
                Err(MemoryError::EntryPoolRunOut) => {
                    if option.is_no_wait() {
                        Err(MemoryError::EntryPoolRunOut)
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
                Err(e) => Err(e),
            };
            break;
        }
        let vm_entry = vm_entry?;
        *vm_entry = source;
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
        Ok(vm_entry)
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

        for i in MIndex::new(0)..size.to_index() {
            let current_virtual_address = virtual_address + i.to_offset();
            let current_physical_address = physical_address + i.to_offset();

            let p_index = (current_virtual_address - vm_entry.get_vm_start_address()).to_index()
                + vm_entry.get_memory_offset().to_index();
            let vm_page;
            loop {
                vm_page = match get_kernel_manager_cluster()
                    .system_memory_manager
                    .alloc_vm_page(
                        current_physical_address,
                        self.is_kernel_virtual_memory_manager(),
                        option,
                    ) {
                    Ok(e) => Ok(e),
                    Err(MemoryError::EntryPoolRunOut) => {
                        if option.is_no_wait() {
                            Err(MemoryError::EntryPoolRunOut)
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
                    Err(e) => Err(e),
                };
                break;
            }
            let vm_page = vm_page?;
            *vm_page = VirtualMemoryPage::new(current_physical_address, p_index);
            vm_page.set_page_status(vm_entry.get_memory_option_flags());
            vm_page.activate();
            vm_entry.get_object_mut().add_vm_page(p_index, vm_page);
        }
        Ok(())
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
        /* Assume user_virtual_address is usable. */
        let user_vm_map_entry = VirtualMemoryEntry::new(
            user_virtual_address,
            kernel_vm_entry
                .get_size()
                .to_end_address(user_virtual_address),
            user_permission,
            user_option,
        );

        let original_vm_object = kernel_vm_entry.get_object_mut();
        if original_vm_object.is_shadow_entry() {
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
        if let Err(e) = vm_object {
            self.lock.unlock();
            user_vm_manager.lock.unlock();
            return Err(e);
        }
        let shared_vm_object = vm_object?;
        let user_vm_map_entry =
            user_vm_manager.insert_vm_map_entry_into_list(user_vm_map_entry, user_option);
        if let Err(e) = user_vm_map_entry {
            get_kernel_manager_cluster()
                .system_memory_manager
                .free_vm_object(shared_vm_object);
            self.lock.unlock();
            user_vm_manager.lock.unlock();
            return Err(e);
        }
        let user_vm_map_entry = user_vm_map_entry?;

        init_struct!(*shared_vm_object, VirtualMemoryObject::new());
        core::mem::swap(shared_vm_object, original_vm_object);

        original_vm_object.set_shared_object(shared_vm_object);
        user_vm_map_entry
            .get_object_mut()
            .set_shared_object(shared_vm_object);

        self.lock.unlock();
        if let Err(e) =
            user_vm_manager._update_page_table_with_vm_entry(user_vm_map_entry, pm_manager, None)
        {
            let _shared_object_lock = shared_vm_object.lock.lock();
            user_vm_map_entry
                .get_object_mut()
                .unset_shared_object(shared_vm_object);
            unsafe { user_vm_manager.vm_entry.remove(&mut user_vm_map_entry.list) };
            user_vm_manager.adjust_vm_entries();
            user_vm_map_entry.set_disabled();
            get_kernel_manager_cluster()
                .system_memory_manager
                .free_vm_entry(user_vm_map_entry);
            drop(_shared_object_lock);
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
        if let Some(next_entry) = vm_entry
            .list
            .get_next(offset_of!(VirtualMemoryEntry, list))
            .map(|e| unsafe { &*e })
        {
            let next_entry_start_address = next_entry.get_vm_start_address();
            if new_size.to_end_address(vm_entry.get_vm_start_address()) >= next_entry_start_address
            {
                return false;
            }
        } else if new_size.to_end_address(vm_entry.get_vm_start_address()) >= MAX_VIRTUAL_ADDRESS {
            return false;
        }

        assert!(old_size >= PAGE_SIZE);
        let old_last_p_index = old_size.to_index() - MIndex::new(1);
        let not_associated_virtual_address = vm_entry.get_vm_end_address() + MSize::new(1);
        let not_associated_physical_address = vm_entry
            .get_object()
            .get_vm_page(old_last_p_index)
            .unwrap()
            .get_physical_address()
            + PAGE_SIZE;

        vm_entry.set_vm_end_address(new_size.to_end_address(vm_entry.get_vm_start_address()));

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
        if let Err(e) = self._update_page_table_with_vm_entry(
            vm_entry,
            pm_manager,
            Some(old_last_p_index + MIndex::new(1)),
        ) {
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
            if !vm_entry.get_memory_option_flags().is_io_map() {
                self.lock.unlock();
                pr_err!("Not mapped entry.");
                return Err(MemoryError::InvalidAddress);
            }
            if self.try_expand_vm_entry(vm_entry, new_size, pm_manager) {
                self.lock.unlock();
                return Ok(virtual_address);
            }
            let permission = vm_entry.get_permission_flags();
            let physical_address = vm_entry
                .get_object()
                .get_vm_page(vm_entry.get_memory_offset().to_index())
                .unwrap()
                .get_physical_address();
            /* Assume: p_index is the first of mapping address */
            let option = vm_entry.get_memory_option_flags();
            vm_entry
                .set_memory_option_flags(option | MemoryOptionFlags::DO_NOT_FREE_PHYSICAL_ADDRESS);
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
            let mut n = 0;
            for index in offset..(offset + number_of_pages) {
                if let Some(p) = vm_entry.get_object().get_vm_page(index) {
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

    fn find_usable_memory_area(&self, size: MSize, option: MemoryOptionFlags) -> Option<VAddress> {
        let (virtual_address_limit_start, virtual_address_limit_end) = if option.is_io_map() {
            (MAP_START_ADDRESS, MAP_END_ADDRESS)
        } else if option.is_alloc_area() {
            (MALLOC_START_ADDRESS, MALLOC_END_ADDRESS)
        } else if option.is_for_user() && option.is_stack() {
            (USER_STACK_START_ADDRESS, USER_STACK_END_ADDRESS)
        } else {
            unimplemented!()
        };
        const OFFSET: usize = offset_of!(VirtualMemoryEntry, list);
        let mut available_start_address = virtual_address_limit_start;

        for e in unsafe { self.vm_entry.iter(OFFSET) } {
            if e.get_vm_end_address() < virtual_address_limit_start {
                continue;
            }
            let end_address = size.to_end_address(available_start_address);
            if end_address > virtual_address_limit_end {
                return None;
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

                return Some(available_start_address);
            }
            available_start_address = e.get_vm_end_address() + MSize::new(1);
        }
        let end_address = size.to_end_address(available_start_address);
        if end_address > virtual_address_limit_end {
            None
        } else {
            Some(available_start_address)
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
            kprintln!(
                "Virtual Address:{:>#16X}, Size:{:>#16X}, W:{:>5}, U:{:>5}, E:{:>5}, Shared:{:>5}",
                entry.get_vm_start_address().to_usize(),
                MSize::from_address(entry.get_vm_start_address(), entry.get_vm_end_address())
                    .to_usize(),
                entry.get_permission_flags().is_writable(),
                entry.get_permission_flags().is_user_accessible(),
                entry.get_permission_flags().is_executable(),
                entry.get_object().is_shadow_entry()
            );
            let first_p_index = entry.get_memory_offset().to_index();
            let last_p_index = MIndex::from_offset(
                entry.get_vm_end_address() - entry.get_vm_start_address()
                    + entry.get_memory_offset(), /* Is it ok? */
            ) + MIndex::new(1);

            let mut omitted = false;
            let mut last_is_not_found = false;
            let mut last_address = PAddress::new(0);

            let object = if let Some(s) = entry.get_object().get_shared_object() {
                &*s
            } else {
                entry.get_object()
            };
            let _lock = if let Ok(l) = object.lock.try_lock() {
                l
            } else {
                pr_warn!("Failed to lock object");
                let next = entry.list.get_next(offset).map(|e| unsafe { &*e });
                if next.is_none() {
                    break;
                }
                entry = next.unwrap();
                continue;
            };
            for i in first_p_index..last_p_index {
                if let Some(p) = object.get_vm_page(i) {
                    if last_is_not_found {
                        kprintln!("...\n - {} Not Found", i.to_usize() - 1);
                        last_is_not_found = false;
                    }
                    if last_address + PAGE_SIZE == p.get_physical_address()
                        && p.get_physical_address().to_usize() != PAGE_SIZE_USIZE
                    {
                        omitted = true;
                        last_address += PAGE_SIZE;
                        continue;
                    } else if omitted {
                        kprintln!(
                            "...\n - {} Physical Address:{:>#16X}",
                            i.to_usize() - 1,
                            last_address.to_usize()
                        );
                        omitted = false;
                    }
                    kprintln!(
                        " - {} Physical Address:{:>#16X}",
                        i.to_usize(),
                        p.get_physical_address().to_usize()
                    );
                    last_address = p.get_physical_address()
                } else if !last_is_not_found {
                    kprintln!(" - {} Not Found", i.to_usize());
                    last_is_not_found = true;
                    last_address = PAddress::new(0);
                }
            }
            if last_is_not_found {
                kprintln!("...\n - {} Not Found(fin)", last_p_index.to_usize() - 1);
            } else if omitted {
                kprintln!(
                    "...\n - {} Physical Address:{:>#16X} (fin)",
                    last_p_index.to_usize() - 1,
                    last_address.to_usize()
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
