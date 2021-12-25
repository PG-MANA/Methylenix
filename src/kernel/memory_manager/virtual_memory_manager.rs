//!
//! Virtual Memory Manager
//!
//! This manager maintains memory map and controls page_manager.
//! The address and size are rounded up to an integral number of PAGE_SIZE.
//!

/* ADD: add physical_memory into reserved_memory_list when it runs out */

mod virtual_memory_entry;
mod virtual_memory_object;
mod virtual_memory_page;

pub(super) use self::virtual_memory_entry::VirtualMemoryEntry;
/*use self::virtual_memory_object::VirtualMemoryObject;*/
pub(super) use self::virtual_memory_page::VirtualMemoryPage;

use super::data_type::{
    Address, MIndex, MOrder, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress, VAddress,
};
use super::physical_memory_manager::PhysicalMemoryManager;
use super::MemoryError;

use crate::arch::target_arch::context::memory_layout::{
    physical_address_to_direct_map, DIRECT_MAP_BASE_ADDRESS, DIRECT_MAP_MAX_SIZE,
    DIRECT_MAP_START_ADDRESS, MALLOC_END_ADDRESS, MALLOC_START_ADDRESS, MAP_END_ADDRESS,
    MAP_START_ADDRESS,
};
use crate::arch::target_arch::paging::{
    PageManager, MAX_VIRTUAL_ADDRESS, PAGE_MASK, PAGE_SIZE, PAGE_SIZE_USIZE,
};

use crate::kernel::collections::ptr_linked_list::PtrLinkedList;

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use core::ops::RangeInclusive;

pub struct VirtualMemoryManager {
    vm_entry: PtrLinkedList<VirtualMemoryEntry>,
    is_system_vm: bool,
    page_manager: PageManager,
}

impl VirtualMemoryManager {
    pub const fn new() -> Self {
        Self {
            vm_entry: PtrLinkedList::new(),
            is_system_vm: false,
            page_manager: PageManager::new(),
        }
    }

    pub fn disable(&mut self) {

        /* TODO: */
    }

    pub const fn is_kernel_virtual_memory_manager(&self) -> bool {
        self.is_system_vm
    }

    pub fn clone_kernel_area(
        &mut self,
        kernel_virtual_memory_manager: &Self,
    ) -> Result<(), MemoryError> {
        assert!(!self.is_kernel_virtual_memory_manager());
        assert!(kernel_virtual_memory_manager.is_kernel_virtual_memory_manager());
        if let Err(e) = self
            .page_manager
            .copy_system_area(&kernel_virtual_memory_manager.page_manager)
        {
            pr_err!("Failed to copy kernel area: {:?}", e);
            return Err(MemoryError::PagingError);
        }
        return Ok(());
    }

    pub fn init_system(
        &mut self,
        max_physical_address: PAddress,
        pm_manager: &mut PhysicalMemoryManager,
    ) {
        self.is_system_vm = true;

        /* Set up page_manager */
        self.page_manager
            .init(pm_manager)
            .expect("Cannot init PageManager");

        self.setup_direct_mapped_area(max_physical_address, pm_manager);
    }

    pub fn init_user(
        &mut self,
        system_virtual_memory_manager: &VirtualMemoryManager,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        self.is_system_vm = false;

        /* Set up page_manager */
        if let Err(e) = self
            .page_manager
            .init_user(&system_virtual_memory_manager.page_manager, pm_manager)
        {
            pr_err!("Failed to init PageManager for user: {:?}", e);
            return Err(MemoryError::PagingError);
        }
        return Ok(());
    }

    #[inline]
    fn check_align(
        physical_address: Option<PAddress>,
        virtual_address: Option<VAddress>,
        size: Option<MSize>,
    ) -> Result<(), MemoryError> {
        if let Some(p) = physical_address {
            if p & !PAGE_MASK != 0 {
                pr_err!("Physical Address({}) is not aligned.", p);
                return Err(MemoryError::NotAligned);
            }
        }
        if let Some(v) = virtual_address {
            if v & !PAGE_MASK != 0 {
                pr_err!("Virtual Address({}) is not aligned.", v);
                return Err(MemoryError::NotAligned);
            }
        }
        if let Some(s) = size {
            if s & !PAGE_MASK != 0 {
                pr_err!("Size({}) is not aligned.", s);
                return Err(MemoryError::NotAligned);
            }
        }
        return Ok(());
    }

    fn setup_direct_mapped_area(
        &mut self,
        max_physical_address: PAddress,
        pm_manager: &mut PhysicalMemoryManager,
    ) {
        let aligned_map_size = MSize::new(
            ((max_physical_address - DIRECT_MAP_BASE_ADDRESS).to_usize() - 1) & PAGE_MASK,
        ) + PAGE_SIZE;
        pr_debug!(
            "Map {} ~ {}",
            DIRECT_MAP_START_ADDRESS,
            aligned_map_size.to_end_address(DIRECT_MAP_START_ADDRESS)
        );
        self.map_address_into_page_table_with_size(
            DIRECT_MAP_BASE_ADDRESS,
            DIRECT_MAP_START_ADDRESS,
            DIRECT_MAP_MAX_SIZE.min(aligned_map_size),
            MemoryPermissionFlags::data(),
            pm_manager,
        )
        .expect("Failed to map physical memory");
    }

    pub fn flush_paging(&mut self) {
        self.page_manager.flush_page_table();
    }

    pub fn update_paging(&mut self /*Not necessary*/, address: VAddress) {
        PageManager::update_page_cache(address);
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
        let vm_start_address = if let Some(address) = self.find_usable_memory_area(size, option) {
            address
        } else {
            pr_warn!("Virtual Address is not available.");
            return Err(MemoryError::AddressNotAvailable);
        };
        self.map_address(
            physical_address,
            Some(vm_start_address),
            size,
            permission,
            option,
            pm_manager,
        )
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
        let entry = if let Some(address) = self.find_usable_memory_area(size, option) {
            VirtualMemoryEntry::new(address, size.to_end_address(address), permission, option)
        } else {
            pr_warn!("Virtual Address is not available.");
            return Err(MemoryError::AddressNotAvailable);
        };

        self.insert_vm_map_entry(entry)
    }

    /// Map virtual_address to physical_address with size.
    ///
    /// This function maps virtual_address to physical_address into vm_entry.
    /// vm_entry must be inserted in [`Self::vm_entry`]. (use the entry from [`alloc_virtual_address`])
    /// virtual_address, physical_address, and size must be page aligned.
    /// This function **does not** update page table.
    pub(super) fn map_physical_address_into_vm_entry(
        &mut self,
        vm_entry: &mut VirtualMemoryEntry,
        virtual_address: VAddress,
        physical_address: PAddress,
        size: MSize,
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
        self._map_address(vm_entry, physical_address, virtual_address, size)
    }

    fn _update_page_table_with_vm_entry(
        &mut self,
        vm_entry: &mut VirtualMemoryEntry,
        pm_manager: &mut PhysicalMemoryManager,
        first_index: Option<MIndex>,
    ) -> Result<(), MemoryError> {
        let first_p_index = first_index.unwrap_or_else(|| vm_entry.get_memory_offset().to_index());
        let last_p_index = MSize::from_address(
            vm_entry.get_vm_start_address(),
            vm_entry.get_vm_end_address(),
        )
        .to_index(); /* OK? */
        for i in first_p_index..=last_p_index {
            if let Some(p) = vm_entry.get_object_mut().get_vm_page_mut(i) {
                p.activate();
                if let Err(e) = self.map_address_into_page_table(
                    p.get_physical_address(),
                    vm_entry.get_vm_start_address() + i.to_offset(), /* OK? */
                    vm_entry.get_permission_flags(),
                    pm_manager,
                ) {
                    pr_err!(
                        "Failed to update paging, address: {}, index: {}: {:?}",
                        vm_entry.get_vm_start_address() + i.to_offset(),
                        i,
                        e
                    );
                    for unassociate_i in first_p_index..i {
                        let address = vm_entry.get_vm_start_address() + unassociate_i.to_offset(); /* OK? */
                        if let Err(u_e) = self.unassociate_address(address, pm_manager) {
                            pr_err!("Failed to rollback paging(VirtualAddress: {}).", address);
                            return Err(u_e);
                        }
                        vm_entry
                            .get_object_mut()
                            .get_vm_page_mut(unassociate_i)
                            .and_then(|p| Some(p.inactivate()));
                    }
                    return Err(e);
                }
            }
        }
        return Ok(());
    }

    /// Apply the mapping of vm_entry into page table
    ///
    /// This function updates [`Self::page_manager`] with the information of vm_entry.
    pub(super) fn update_page_table_with_vm_entry(
        &mut self,
        vm_entry: &mut VirtualMemoryEntry,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        self._update_page_table_with_vm_entry(vm_entry, pm_manager, None)
    }

    pub fn map_address(
        &mut self,
        physical_address: PAddress,
        virtual_address: Option<VAddress>,
        size: MSize,
        permission: MemoryPermissionFlags,
        mut option: MemoryOptionFlags,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<VAddress, MemoryError> {
        Self::check_align(Some(physical_address), virtual_address, Some(size))?;

        if !option.is_for_kernel() && !option.is_for_user() {
            option = option | MemoryOptionFlags::KERNEL;
        }
        if option.is_for_kernel() && permission.is_user_accessible() {
            pr_err!("Invalid Memory Permission");
            return Err(MemoryError::InternalError);
        }

        let mut entry = if let Some(vm_start_address) = virtual_address {
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

        let vm_start_address = entry.get_vm_start_address();
        self._map_address(&mut entry, physical_address, vm_start_address, size)?;

        let entry = self.insert_vm_map_entry(entry)?;

        if entry.get_memory_option_flags().is_io_map()
            || entry.get_memory_option_flags().is_memory_map()
        {
            if let Err(e) = self.map_address_into_page_table_with_size(
                physical_address,
                vm_start_address,
                size,
                permission,
                pm_manager,
            ) {
                pr_err!("Failed to map address(VirtualAddress:{}, PhysicalAddress: {}) with block_size: {:?}",vm_start_address,physical_address, e);
                if let Err(e) =
                    self.unassociate_address_with_size(vm_start_address, size, pm_manager)
                {
                    pr_err!(
                        "Failed to unmap address(VirtualAddress: {}): {:?}",
                        vm_start_address,
                        e
                    );
                }
                self.vm_entry.remove(&mut entry.list);
                get_kernel_manager_cluster()
                    .system_memory_manager
                    .free_vm_entry(entry);
                return Err(MemoryError::PagingError);
            }
            /* TODO: check the page_table is used currently. */
            for i in MIndex::new(0)..size.to_index() {
                self.update_paging(vm_start_address + i.to_offset());
            }
        } else {
            for i in MIndex::new(0)..size.to_index() {
                if let Err(e) = self.map_address_into_page_table(
                    physical_address + i.to_offset(),
                    vm_start_address + i.to_offset(),
                    permission,
                    pm_manager,
                ) {
                    pr_err!(
                        "Failed to map address(VirtualAddress:{}, PhysicalAddress: {}): {:?}",
                        vm_start_address + i.to_offset(),
                        physical_address + i.to_offset(),
                        e
                    );

                    for u_i in MIndex::new(0)..i {
                        if let Err(e) =
                            self.unassociate_address(vm_start_address + u_i.to_offset(), pm_manager)
                        {
                            pr_err!(
                                "Failed to unmap address(VirtualAddress: {}): {:?}",
                                vm_start_address + i.to_offset(),
                                e
                            );
                        }
                    }
                    self.vm_entry.remove(&mut entry.list);
                    get_kernel_manager_cluster()
                        .system_memory_manager
                        .free_vm_entry(entry);
                    return Err(MemoryError::PagingError);
                }
                /* TODO: check the page_table is used currently. */
                self.update_paging(vm_start_address + i.to_offset());
            }
        }
        Ok(vm_start_address)
    }

    pub fn io_map(
        &mut self,
        physical_address: PAddress,
        virtual_address: Option<VAddress>,
        size: MSize,
        permission: MemoryPermissionFlags,
        option: Option<MemoryOptionFlags>,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<VAddress, MemoryError> {
        assert_eq!(permission.is_executable(), false); /* Disallow executing code on device mapping */
        self.map_address(
            physical_address,
            virtual_address,
            size,
            permission,
            option.unwrap_or(MemoryOptionFlags::KERNEL) | MemoryOptionFlags::IO_MAP,
            pm_manager,
        )
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
        if let Some(vm_map_entry) = self.find_entry_mut(vm_start_address) {
            self._free_address(vm_map_entry, pm_manager)
        } else {
            pr_err!("Cannot find vm_entry.");
            Err(MemoryError::InvalidAddress)
        }
    }

    pub(super) fn _free_address(
        &mut self,
        vm_entry /* will be removed from list and freed */: &'static mut VirtualMemoryEntry,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        let first_p_index = vm_entry.get_memory_offset().to_index();
        let last_p_index = MSize::from_address(
            vm_entry.get_vm_start_address(),
            vm_entry.get_vm_end_address(),
        )
        .to_index();

        if vm_entry.get_memory_option_flags().is_io_map()
            || vm_entry.get_memory_option_flags().is_memory_map()
        {
            if let Err(e) = self.unassociate_address_with_size(
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
                return Err(MemoryError::PagingError);
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
                            .free_vm_page(p);
                    }
                }
            }
        } else {
            for i in first_p_index..=last_p_index {
                if let Some(p) = vm_entry.get_object_mut().remove_vm_page(i) {
                    if let Err(e) = self.unassociate_address(
                        vm_entry.get_vm_start_address() + i.to_offset(),
                        pm_manager,
                    ) {
                        pr_err!(
                            "Failed to unmap address({}): {:?}",
                            vm_entry.get_vm_start_address() + i.to_offset(),
                            e
                        );
                        return Err(MemoryError::PagingError);
                    }
                    if !vm_entry
                        .get_memory_option_flags()
                        .should_not_free_phy_address()
                    {
                        if let Err(e) = pm_manager.free(p.get_physical_address(), PAGE_SIZE, false)
                        {
                            pr_err!("Failed to free physical memory: {:?}", e);
                        }
                    }
                    get_kernel_manager_cluster()
                        .system_memory_manager
                        .free_vm_page(p);
                }
            }
        }

        self.vm_entry.remove(&mut vm_entry.list);
        self.adjust_vm_entries();
        vm_entry.set_disabled();
        get_kernel_manager_cluster()
            .system_memory_manager
            .free_vm_entry(vm_entry);

        return Ok(());
    }

    /// Allocate VirtualMemoryEntry from the pool and chain it into [`Self::vm_entry`]
    fn insert_vm_map_entry(
        &mut self,
        source: VirtualMemoryEntry,
    ) -> Result<&'static mut VirtualMemoryEntry, MemoryError> {
        let entry = get_kernel_manager_cluster()
            .system_memory_manager
            .alloc_vm_entry(self.is_system_vm)?;
        *entry = source;
        if self.vm_entry.is_empty() {
            self.vm_entry.insert_head(&mut entry.list);
        } else if let Some(prev_entry) = self.find_previous_entry_mut(entry.get_vm_start_address())
        {
            self.vm_entry
                .insert_after(&mut prev_entry.list, &mut entry.list);
        } else if entry.get_vm_end_address()
            < unsafe {
                self.vm_entry
                    .get_first_entry(offset_of!(VirtualMemoryEntry, list))
            }
            .unwrap()
            .get_vm_start_address()
        {
            self.vm_entry.insert_head(&mut entry.list);
        } else {
            pr_err!("Cannot insert Virtual Memory Entry.");
            return Err(MemoryError::InternalError);
        }
        self.adjust_vm_entries();
        return Ok(entry);
    }

    /// Insert pages into VirtualMemoryEntry without applying PageManager.
    ///
    /// This function allocate vm_page and inserts it into vm_entry.
    /// virtual_address must be allocated.
    fn _map_address(
        &mut self,
        vm_entry: &mut VirtualMemoryEntry,
        physical_address: PAddress,
        virtual_address: VAddress,
        size: MSize,
    ) -> Result<(), MemoryError> {
        assert_eq!(physical_address & !PAGE_MASK, 0);
        assert_eq!(virtual_address & !PAGE_MASK, 0);
        assert_eq!(size & !PAGE_MASK, 0);
        assert!(!size.is_zero());

        for i in MIndex::new(0)..size.to_index() {
            let current_virtual_address = virtual_address + i.to_offset();
            let current_physical_address = physical_address + i.to_offset();

            let p_index = (current_virtual_address - vm_entry.get_vm_start_address()).to_index()
                + vm_entry.get_memory_offset().to_index();
            let vm_page = get_kernel_manager_cluster()
                .system_memory_manager
                .alloc_vm_page(self.is_system_vm)?;

            *vm_page = VirtualMemoryPage::new(current_physical_address, p_index);
            vm_page.set_page_status(vm_entry.get_memory_option_flags());
            vm_page.activate();
            vm_entry.get_object_mut().add_vm_page(p_index, vm_page);
        }
        return Ok(());
    }

    fn map_address_into_page_table(
        &mut self,
        physical_address: PAddress,
        virtual_address: VAddress,
        permission: MemoryPermissionFlags,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        return self
            .page_manager
            .associate_address(pm_manager, physical_address, virtual_address, permission)
            .or(Err(MemoryError::PagingError));
    }

    fn map_address_into_page_table_with_size(
        &mut self,
        physical_address: PAddress,
        virtual_address: VAddress,
        size: MSize,
        permission: MemoryPermissionFlags,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        return self
            .page_manager
            .associate_area(
                pm_manager,
                physical_address,
                virtual_address,
                size,
                permission,
            )
            .or(Err(MemoryError::PagingError));
    }

    fn unassociate_address(
        &self,
        virtual_address: VAddress,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        match self
            .page_manager
            .unassociate_address(virtual_address, pm_manager, false)
        {
            Ok(()) => Ok(()),
            Err(e) => {
                pr_err!("Cannot unassociate memory Err:{:?}", e);
                Err(MemoryError::PagingError)
            }
        }
    }

    fn unassociate_address_with_size(
        &self,
        virtual_address: VAddress,
        size: MSize,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        match self.page_manager.unassociate_address_width_size(
            virtual_address,
            size,
            pm_manager,
            true,
        ) {
            Ok(()) => Ok(()),
            Err(e) => {
                pr_err!("Failed to unmap memory Err:{:?}", e);
                Err(MemoryError::PagingError)
            }
        }
    }

    fn try_expand_vm_entry(
        &mut self,
        vm_entry: &mut VirtualMemoryEntry,
        new_size: MSize,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> bool {
        let old_size = vm_entry.get_size();
        if old_size >= new_size {
            return true;
        }
        if let Some(next_entry) =
            unsafe { vm_entry.list.get_next(offset_of!(VirtualMemoryEntry, list)) }
        {
            let next_entry_start_address =
                unsafe { &*(next_entry as *const VirtualMemoryEntry) }.get_vm_start_address();
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

        if let Err(e) = self._map_address(
            vm_entry,
            not_associated_physical_address,
            not_associated_virtual_address,
            new_size - old_size,
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
        return true;
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
        if let Some(entry) = self.find_entry_mut(virtual_address) {
            if !(entry.get_memory_option_flags().is_io_map()
                || entry.get_memory_option_flags().is_memory_map())
            {
                pr_err!("Not mapped entry.");
                return Err(MemoryError::InvalidAddress);
            }
            if self.try_expand_vm_entry(entry, new_size, pm_manager) {
                return Ok(virtual_address);
            }
            let permission = entry.get_permission_flags();
            let physical_address = entry
                .get_object()
                .get_vm_page(entry.get_memory_offset().to_index())
                .unwrap()
                .get_physical_address();
            /* Assume: p_index is the first of mapping address */
            let option = entry.get_memory_option_flags();
            entry.set_memory_option_flags(option | MemoryOptionFlags::DO_NOT_FREE_PHYSICAL_ADDRESS);
            self._free_address(entry, pm_manager)?;
            self.map_address(
                physical_address,
                None,
                new_size,
                permission,
                option,
                pm_manager,
            )
        } else {
            Err(MemoryError::InvalidAddress)
        }
    }

    pub(super) fn add_physical_memory_manager_pool(
        &mut self,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        let address = pm_manager
            .alloc(PAGE_SIZE, MOrder::new(0))
            .expect("Failed to alloc memory");
        pm_manager.add_memory_entry_pool(
            physical_address_to_direct_map(address).to_usize(),
            PAGE_SIZE_USIZE,
        );
        return Ok(());
    }

    fn _find_entry(&self, vm_address: VAddress) -> Option<&'static VirtualMemoryEntry> {
        for e in unsafe { self.vm_entry.iter(offset_of!(VirtualMemoryEntry, list)) } {
            if e.get_vm_start_address() <= vm_address && e.get_vm_end_address() >= vm_address {
                return Some(e);
            }
        }
        None
    }

    fn find_entry_mut(&mut self, vm_address: VAddress) -> Option<&'static mut VirtualMemoryEntry> {
        for e in unsafe { self.vm_entry.iter_mut(offset_of!(VirtualMemoryEntry, list)) } {
            if e.get_vm_start_address() <= vm_address && e.get_vm_end_address() >= vm_address {
                return Some(e);
            }
        }
        None
    }

    fn find_previous_entry_mut(
        &mut self,
        vm_address: VAddress,
    ) -> Option<&'static mut VirtualMemoryEntry> {
        const OFFSET: usize = offset_of!(VirtualMemoryEntry, list);
        for e in unsafe { self.vm_entry.iter_mut(OFFSET) } {
            if e.get_vm_start_address() > vm_address {
                return unsafe { e.list.get_prev_mut(OFFSET) };
            } else if !e.list.has_next() && e.get_vm_end_address() < vm_address {
                return Some(e);
            }
        }
        None
    }

    pub fn find_usable_memory_area(
        &self,
        size: MSize,
        option: MemoryOptionFlags,
    ) -> Option<VAddress> {
        let (virtual_address_limit_start, virtual_address_limit_end) =
            if option.is_memory_map() || option.is_io_map() {
                (MAP_START_ADDRESS, MAP_END_ADDRESS)
            } else if option.is_alloc_area() {
                (MALLOC_START_ADDRESS, MALLOC_END_ADDRESS)
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
                assert!(unsafe { e.list.get_next(OFFSET) }
                    .and_then(|n| Some(!Self::is_overlapped(
                        &(available_start_address..=end_address),
                        &(n.get_vm_start_address()..=n.get_vm_end_address())
                    )))
                    .unwrap_or(true));
                assert!(unsafe { e.list.get_prev(OFFSET) }
                    .and_then(|p| Some(!Self::is_overlapped(
                        &(available_start_address..=end_address),
                        &(p.get_vm_start_address()..=p.get_vm_end_address())
                    )))
                    .unwrap_or(true));

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
            || range_2.contains(&range_1.start())
            || range_2.contains(&range_1.end())
    }

    pub fn dump_memory_manager(
        &self,
        start_vm_address: Option<VAddress>,
        end_vm_address: Option<VAddress>,
    ) {
        let start = start_vm_address.unwrap_or(VAddress::new(0));
        let end = end_vm_address.unwrap_or(MAX_VIRTUAL_ADDRESS);
        kprintln!("Is system's vm :{}", self.is_system_vm);
        if self.vm_entry.is_empty() {
            kprintln!("There is no root entry.");
            return;
        }
        let offset = offset_of!(VirtualMemoryEntry, list);
        let mut entry = unsafe { self.vm_entry.get_first_entry(offset) }.unwrap();
        loop {
            if entry.get_vm_start_address() < start || entry.get_vm_end_address() > end {
                let next = unsafe { entry.list.get_next(offset) };
                if next.is_none() {
                    break;
                }
                entry = next.unwrap();
                continue;
            }
            kprintln!(
                "Virtual Address:{:>#16X}, Size:{:>#16X}, W:{:>5}, U:{:>5}, E:{:>5}",
                entry.get_vm_start_address().to_usize(),
                MSize::from_address(entry.get_vm_start_address(), entry.get_vm_end_address())
                    .to_usize(),
                entry.get_permission_flags().is_writable(),
                entry.get_permission_flags().is_user_accessible(),
                entry.get_permission_flags().is_executable()
            );
            let first_p_index = entry.get_memory_offset().to_index();
            let last_p_index = MIndex::from_offset(
                entry.get_vm_end_address() - entry.get_vm_start_address()
                    + entry.get_memory_offset(), /* Is it ok? */
            ) + MIndex::new(1);

            let mut omitted = false;
            let mut last_is_not_found = false;
            let mut last_address = PAddress::new(0);

            for i in first_p_index..last_p_index {
                if let Some(p) = entry.get_object().get_vm_page(i) {
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
            let next = unsafe { entry.list.get_next(offset) };
            if next.is_none() {
                break;
            }
            kprintln!(""); // \n
            entry = next.unwrap();
        }
        kprintln!("----Page Manager----");
        self.page_manager
            .dump_table(start_vm_address, end_vm_address);
    }
}
