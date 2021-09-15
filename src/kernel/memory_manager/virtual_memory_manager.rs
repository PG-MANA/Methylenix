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

pub use self::virtual_memory_entry::VirtualMemoryEntry;
/*use self::virtual_memory_object::VirtualMemoryObject;*/
use self::virtual_memory_page::VirtualMemoryPage;

use super::data_type::{
    Address, MIndex, MOrder, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress, VAddress,
};
use super::physical_memory_manager::PhysicalMemoryManager;
use super::slab_allocator::pool_allocator::PoolAllocator;
use super::MemoryError;

use crate::arch::target_arch::context::memory_layout::{
    MALLOC_END_ADDRESS, MALLOC_START_ADDRESS, MAP_END_ADDRESS, MAP_START_ADDRESS,
};
use crate::arch::target_arch::paging::{
    PageManager, PagingError, MAX_VIRTUAL_ADDRESS, PAGE_MASK, PAGE_SHIFT, PAGE_SIZE,
    PAGE_SIZE_USIZE, PAGING_CACHE_LENGTH,
};

use crate::kernel::collections::ptr_linked_list::PtrLinkedList;

use core::ops::RangeInclusive;

pub struct VirtualMemoryManager {
    vm_entry: PtrLinkedList<VirtualMemoryEntry>,
    is_system_vm: bool,
    page_manager: PageManager,
    vm_entry_pool: PoolAllocator<VirtualMemoryEntry>,
    /*vm_object_pool: PoolAllocator<VirtualMemoryObject>,*/
    vm_page_pool: PoolAllocator<VirtualMemoryPage>,
    reserved_memory_list: PoolAllocator<[u8; PAGE_SIZE_USIZE]>,
    direct_mapped_area: Option<DirectMappedArea>, /* think algorithm */
}

struct DirectMappedArea {
    allocator: PhysicalMemoryManager,
    start_address: VAddress,
    end_address: VAddress,
}

impl VirtualMemoryManager {
    const VM_MAP_ENTRY_POOL_SIZE: MSize = PAGE_SIZE << MSize::new(3);
    /*const VM_OBJECT_POOL_SIZE: usize = PAGE_SIZE * 8;*/
    const VM_PAGE_POOL_SIZE: MSize = PAGE_SIZE << MSize::new(7);

    const VM_MAP_ENTRY_CACHE_LEN: usize = 12;
    const VM_PAGE_CACHE_LEN: usize = 12;

    pub const fn new() -> Self {
        Self {
            vm_entry: PtrLinkedList::new(),
            is_system_vm: false,
            page_manager: PageManager::new(),
            vm_entry_pool: PoolAllocator::new(),
            /*vm_object_pool: PoolAllocator::new(),*/
            vm_page_pool: PoolAllocator::new(),
            reserved_memory_list: PoolAllocator::new(),
            direct_mapped_area: None,
        }
    }

    pub fn init(&mut self, is_system_vm: bool, pm_manager: &mut PhysicalMemoryManager) {
        //MEMO: 勝手にダイレクトマッピング?(ストレートマッピング?)をしているが、
        //      システム起動時・プロセス起動時にダイレクトマッピングされており且つ
        //      PhysicalMemoryManagerにおいて先に利用されているメモリ領域が予約されていることに依存している。

        self.is_system_vm = is_system_vm;

        /* Set up cache list */
        let mut reserved_memory_list = unsafe {
            core::mem::MaybeUninit::<[PAddress; PAGING_CACHE_LENGTH]>::uninit().assume_init()
        };
        for i in 0..PAGING_CACHE_LENGTH {
            let cache_address = pm_manager
                .alloc(PAGE_SIZE, MOrder::new(PAGE_SHIFT))
                .expect("Cannot alloc memory for paging cache");
            reserved_memory_list[i] = cache_address;
            self.reserved_memory_list
                .free(unsafe { &mut *(cache_address.to_usize() as *mut [u8; PAGE_SIZE_USIZE]) });
        }

        /* Set up page_manager */
        self.page_manager
            .init(&mut self.reserved_memory_list)
            .expect("Cannot init PageManager");

        self.setup_pools(pm_manager);

        /* Insert cached_memory_list entry */
        for i in 0..PAGING_CACHE_LENGTH {
            let cache_address = reserved_memory_list[i];
            let mut entry = VirtualMemoryEntry::new(
                cache_address.to_direct_mapped_v_address(),
                PAGE_SIZE.to_end_address(cache_address.to_direct_mapped_v_address()),
                MemoryPermissionFlags::data(),
                MemoryOptionFlags::KERNEL | MemoryOptionFlags::WIRED,
            );
            self._map_address(
                &mut entry,
                cache_address,
                cache_address.to_direct_mapped_v_address(),
                PAGE_SIZE,
                pm_manager,
            )
            .expect("Cannot map address for paging cache");

            self.insert_vm_map_entry(entry, pm_manager)
                .expect("Cannot insert Virtual Memory Entry for paging cache");
            self.map_address_into_page_table(
                cache_address,
                cache_address.to_direct_mapped_v_address(),
                MemoryPermissionFlags::data(),
                pm_manager,
            )
            .expect("Cannot associate address for paging cache");
        }

        self.setup_direct_mapped_area(pm_manager);
    }

    fn setup_pools(&mut self, pm_manager: &mut PhysicalMemoryManager) {
        let alloc_func = |size: MSize, p: &mut PhysicalMemoryManager| -> VAddress {
            p.alloc(size, MOrder::new(PAGE_SHIFT))
                .and_then(|p| Ok(p.to_direct_mapped_v_address()))
                .expect("Failed to allocate memory")
        };
        let map_func = |vm_manager: &mut Self,
                        name: &str,
                        address: VAddress,
                        size: MSize,
                        p: &mut PhysicalMemoryManager| {
            assert_eq!((address & !PAGE_MASK), 0);
            assert_eq!((size & !PAGE_MASK), 0);

            let mut entry = VirtualMemoryEntry::new(
                address,
                size.to_end_address(address),
                MemoryPermissionFlags::data(),
                MemoryOptionFlags::KERNEL | MemoryOptionFlags::WIRED,
            );
            if let Err(e) = vm_manager._map_address(
                &mut entry,
                address.to_direct_mapped_p_address(),
                address,
                size,
                p,
            ) {
                panic!("Cannot map address for {} Err:{:?}", name, e);
            }
            vm_manager
                .insert_vm_map_entry(entry, p)
                .expect("Cannot insert Virtual Memory Entry");

            for i in MIndex::new(0)..size.to_index() {
                if let Err(e) = vm_manager.map_address_into_page_table(
                    address.to_direct_mapped_p_address() + i.to_offset(),
                    address + i.to_offset(),
                    MemoryPermissionFlags::data(),
                    p,
                ) {
                    panic!("Cannot associate address for {} Err:{:?}", name, e);
                }
            }
        };

        let vm_map_entry_pool_address = alloc_func(Self::VM_MAP_ENTRY_POOL_SIZE, pm_manager);
        /*let vm_object_pool_address = alloc_func(Self::VM_OBJECT_POOL_SIZE, "vm_object", pm_manager);*/
        let vm_page_pool_address = alloc_func(Self::VM_PAGE_POOL_SIZE, pm_manager);

        unsafe {
            self.vm_entry_pool.add_pool(
                vm_map_entry_pool_address.to_usize(),
                Self::VM_MAP_ENTRY_POOL_SIZE.to_usize(),
            )
        };
        /*self.vm_object_pool
        .set_initial_pool(vm_object_pool_address, Self::VM_OBJECT_POOL_SIZE);*/
        unsafe {
            self.vm_page_pool.add_pool(
                vm_page_pool_address.to_usize(),
                Self::VM_PAGE_POOL_SIZE.to_usize(),
            )
        };

        map_func(
            self,
            "vm_entry",
            vm_map_entry_pool_address,
            Self::VM_MAP_ENTRY_POOL_SIZE,
            pm_manager,
        );
        /*map_func(
            self,
            "vm_object",
            vm_object_pool_address,
            Self::VM_OBJECT_POOL_SIZE,
            pm_manager,
        );*/
        map_func(
            self,
            "vm_page",
            vm_page_pool_address,
            Self::VM_PAGE_POOL_SIZE,
            pm_manager,
        );
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

    fn setup_direct_mapped_area(&mut self, pm_manager: &mut PhysicalMemoryManager) {
        /* Direct mapped area is used for page table or io map(needs DMA) (object pools should not use this) */
        /* When use direct mapped area, you must map address into direct_mapped_area.entry. */
        let direct_mapped_area_size =
            MSize::new((pm_manager.get_free_memory_size().to_usize() / 20) & PAGE_MASK); /* temporary */
        assert!(MSize::new(2 << PAGE_SHIFT) < direct_mapped_area_size);
        let direct_mapped_area_address = pm_manager
            .alloc(direct_mapped_area_size, MOrder::new(PAGE_SHIFT))
            .expect("Cannot alloc memory for direct map.");

        pr_info!(
            "{} ~ {} (size: {}) are reserved for direct map",
            direct_mapped_area_address,
            direct_mapped_area_size.to_end_address(direct_mapped_area_address),
            direct_mapped_area_size
        );

        self.map_address_into_page_table_with_size(
            direct_mapped_area_address,
            direct_mapped_area_address.to_direct_mapped_v_address(),
            direct_mapped_area_size,
            MemoryPermissionFlags::data(),
            pm_manager,
        )
        .expect("Cannot associate address for direct map");

        let mut direct_mapped_area_allocator = PhysicalMemoryManager::new();
        direct_mapped_area_allocator
            .add_memory_entry_pool(direct_mapped_area_address.to_usize(), 2 << PAGE_SHIFT);
        direct_mapped_area_allocator
            .free(
                direct_mapped_area_address + MSize::new(2 << PAGE_SHIFT),
                direct_mapped_area_size - MSize::new(2 << PAGE_SHIFT),
                true,
            )
            .expect("Failed to free direct mapped area.");

        self.map_memory_from_direct_map(
            direct_mapped_area_address,
            MSize::new(2 << PAGE_SHIFT),
            pm_manager,
        )
        .expect("Cannot insert vm_entry for direct mapped area allocator.");

        self.direct_mapped_area = Some(DirectMappedArea {
            allocator: direct_mapped_area_allocator,
            start_address: direct_mapped_area_address.to_direct_mapped_v_address(),
            end_address: direct_mapped_area_size
                .to_end_address(direct_mapped_area_address)
                .to_direct_mapped_v_address(),
        });

        /*
        let aligned_memory_size =
            MSize::new((pm_manager.get_memory_size().to_usize() - 1) & PAGE_MASK) + PAGE_SIZE;
        self.map_address_into_page_table_with_size(
            PAddress::new(0),
            DIRECT_MAP_START_ADDRESS,
            MSize::from_address(DIRECT_MAP_START_ADDRESS, DIRECT_MAP_END_ADDRESS)
                .min(aligned_memory_size),
            MemoryPermissionFlags::data(),
            pm_manager,
        )
        .expect("Failed to direct map");*/
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
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<&'static mut VirtualMemoryEntry, MemoryError> {
        Self::check_align(None, None, Some(size))?;
        let entry = if let Some(address) = self.find_usable_memory_area(size, option) {
            VirtualMemoryEntry::new(address, size.to_end_address(address), permission, option)
        } else {
            pr_warn!("Virtual Address is not available.");
            return Err(MemoryError::AddressNotAvailable);
        };

        self.insert_vm_map_entry(entry, pm_manager)
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
        self._map_address(
            vm_entry,
            physical_address,
            virtual_address,
            size,
            pm_manager,
        )
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
        self.check_object_pools(pm_manager)?;
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
        self._map_address(
            &mut entry,
            physical_address,
            vm_start_address,
            size,
            pm_manager,
        )?;

        let entry = self.insert_vm_map_entry(entry, pm_manager)?;

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
                self.vm_entry_pool.free(entry);
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
                    self.vm_entry_pool.free(entry);
                    return Err(MemoryError::PagingError);
                }
                /* TODO: check the page_table is used currently. */
                self.update_paging(vm_start_address + i.to_offset());
            }
        }
        self.check_object_pools(pm_manager)?;
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
                    if let Err(e) = pm_manager.free(p.get_physical_address(), PAGE_SIZE, false) {
                        pr_err!("Failed to free physical memory: {:?}", e);
                    }
                }
                self.vm_page_pool.free(p);
            }
        }
        if vm_entry.get_memory_option_flags().is_direct_mapped() {
            if let Err(e) = self.direct_mapped_area.as_mut().unwrap().allocator.free(
                vm_entry.get_vm_start_address().to_direct_mapped_p_address(),
                vm_entry.get_size(),
                false,
            ) {
                pr_err!("Failed to free direct mapped area: {:?}", e);
            }
        }

        self.vm_entry.remove(&mut vm_entry.list);
        self.adjust_vm_entries();
        vm_entry.set_disabled();
        self.vm_entry_pool.free(vm_entry);

        return Ok(());
    }

    /// Allocate VirtualMemoryEntry from the pool and chain it into [`Self::vm_entry`]
    fn insert_vm_map_entry(
        &mut self,
        source: VirtualMemoryEntry,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<&'static mut VirtualMemoryEntry, MemoryError> {
        let entry = match self.vm_entry_pool.alloc() {
            Ok(e) => e,
            Err(_) => {
                self.add_vm_entry_pool(Some(pm_manager))?;
                self.vm_entry_pool
                    .alloc()
                    .or(Err(MemoryError::EntryPoolRunOut))?
            }
        };
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
        pm_manager: &mut PhysicalMemoryManager,
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
            let vm_page = match self.vm_page_pool.alloc() {
                Ok(p) => p,
                Err(_) => {
                    self.add_vm_page_pool(Some(pm_manager))?;
                    self.vm_page_pool
                        .alloc()
                        .or(Err(MemoryError::EntryPoolRunOut))?
                }
            };
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
        loop {
            match self.page_manager.associate_address(
                &mut self.reserved_memory_list,
                physical_address,
                virtual_address,
                permission,
            ) {
                Ok(()) => {
                    return Ok(());
                }
                Err(PagingError::MemoryCacheRanOut) => {
                    for _ in 0..PAGING_CACHE_LENGTH {
                        match self.alloc_from_direct_map(PAGE_SIZE, pm_manager) {
                            Ok(address) => self
                                .reserved_memory_list
                                .free_ptr(address.to_usize() as *mut _),
                            Err(e) => {
                                pr_err!("Failed to allocate memory for paging: {:?}", e);
                                return Err(MemoryError::PagingError);
                            }
                        }
                    }
                    /* retry (by loop) */
                }
                Err(e) => {
                    pr_err!(
                        "Failed to map VirtualAddress({}) into page table: {:?}",
                        virtual_address,
                        e
                    );
                    return Err(MemoryError::PagingError);
                }
            };
        }
    }

    fn map_address_into_page_table_with_size(
        &mut self,
        physical_address: PAddress,
        virtual_address: VAddress,
        size: MSize,
        permission: MemoryPermissionFlags,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        loop {
            match self.page_manager.associate_area(
                &mut self.reserved_memory_list,
                physical_address,
                virtual_address,
                size,
                permission,
            ) {
                Ok(()) => {
                    return Ok(());
                }
                Err(PagingError::MemoryCacheRanOut) => {
                    for _ in 0..PAGING_CACHE_LENGTH {
                        match self.alloc_from_direct_map(PAGE_SIZE, pm_manager) {
                            Ok(address) => self
                                .reserved_memory_list
                                .free_ptr(address.to_usize() as *mut _),
                            Err(e) => {
                                pr_err!("Failed to allocate memory for paging: {:?}", e);
                                return Err(MemoryError::PagingError);
                            }
                        }
                    }
                    /* retry (by loop) */
                }
                Err(e) => {
                    pr_err!(
                        "Failed to map VirtualAddress({}) into page table: {:?}",
                        virtual_address,
                        e
                    );
                    return Err(MemoryError::PagingError);
                }
            };
        }
    }

    fn unassociate_address(
        &mut self,
        virtual_address: VAddress,
        _pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        match self.page_manager.unassociate_address(
            virtual_address,
            &mut self.reserved_memory_list,
            false,
        ) {
            Ok(()) => Ok(()),
            Err(e) => {
                pr_err!("Cannot unassociate memory Err:{:?}", e);
                Err(MemoryError::PagingError)
            }
        }
    }

    fn unassociate_address_with_size(
        &mut self,
        virtual_address: VAddress,
        size: MSize,
        _pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        match self.page_manager.unassociate_address_width_size(
            virtual_address,
            size,
            &mut self.reserved_memory_list,
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
            pm_manager,
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

    fn add_entry_pool(
        &mut self,
        func_to_add_pool: &dyn Fn(&mut Self, VAddress, MSize),
        current_pool_len: usize,
        new_pool_size: MSize,
        mut pm_manager: Option<&mut PhysicalMemoryManager>,
    ) -> Result<(), MemoryError> {
        if current_pool_len > 0 && pm_manager.is_some() {
            let pm_manager = pm_manager.as_mut().unwrap();
            match pm_manager.alloc(new_pool_size, MOrder::new(PAGE_SHIFT)) {
                Ok(address) => {
                    match self.alloc_and_map_virtual_address(
                        new_pool_size,
                        address,
                        MemoryPermissionFlags::data(),
                        MemoryOptionFlags::ALLOC,
                        pm_manager,
                    ) {
                        Ok(v_address) => {
                            self.update_paging(v_address);
                            func_to_add_pool(self, v_address, new_pool_size);
                            return Ok(());
                        }
                        Err(e) => {
                            pr_err!("Failed to allocate vm_entry's pool: {:?}", e);
                            if let Err(e) = pm_manager.free(address, new_pool_size, false) {
                                pr_err!(
                                    "Failed to free physical memory for VirtualMemoryManager: {:?}",
                                    e
                                );
                            }
                        }
                    }
                }
                Err(MemoryError::EntryPoolRunOut) => {
                    return if let Err(e) = self.add_physical_memory_manager_pool(pm_manager) {
                        pr_err!("Failed to add PhysicalMemoryManager's memory pool: {:?}", e);
                        Err(e)
                    } else {
                        self.add_entry_pool(
                            func_to_add_pool,
                            current_pool_len,
                            new_pool_size,
                            Some(pm_manager),
                        )
                    };
                }
                Err(e) => {
                    pr_err!("Failed to add PhysicalMemoryManager's memory pool: {:?}", e);
                    return Err(e);
                }
            }
        }
        /* Allocate from Direct Mapped Area */
        let address = self.alloc_address_from_direct_map(PAGE_SIZE)?;
        func_to_add_pool(self, address.to_direct_mapped_v_address(), PAGE_SIZE);

        self.map_memory_from_direct_map(
            address,
            PAGE_SIZE,
            pm_manager.unwrap_or(&mut PhysicalMemoryManager::new() /* Temporary*/),
        )?;
        pr_info!("Allocated vm_entry pool from direct mapped area.");
        return Ok(());
    }

    fn add_vm_entry_pool(
        &mut self,
        pm_manager: Option<&mut PhysicalMemoryManager>,
    ) -> Result<(), MemoryError> {
        let f = |m: &mut Self, a: VAddress, s: MSize| unsafe {
            m.vm_entry_pool.add_pool(a.to_usize(), s.to_usize())
        };
        self.add_entry_pool(
            &f,
            self.vm_entry_pool.get_count(),
            Self::VM_MAP_ENTRY_POOL_SIZE,
            pm_manager,
        )
    }

    fn add_vm_page_pool(
        &mut self,
        pm_manager: Option<&mut PhysicalMemoryManager>,
    ) -> Result<(), MemoryError> {
        let f = |m: &mut Self, a: VAddress, s: MSize| unsafe {
            m.vm_page_pool.add_pool(a.to_usize(), s.to_usize())
        };
        self.add_entry_pool(
            &f,
            self.vm_entry_pool.get_count(),
            Self::VM_PAGE_POOL_SIZE,
            pm_manager,
        )
    }

    fn check_object_pools(
        &mut self,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        if self.vm_page_pool.get_count() < Self::VM_PAGE_CACHE_LEN {
            self.add_vm_page_pool(Some(pm_manager))?;
        }
        if self.vm_entry_pool.get_count() < Self::VM_MAP_ENTRY_CACHE_LEN {
            self.add_vm_entry_pool(Some(pm_manager))?;
        }
        return Ok(());
    }

    fn alloc_address_from_direct_map(&mut self, size: MSize) -> Result<PAddress, MemoryError> {
        Self::check_align(None, None, Some(size))?;
        if self.direct_mapped_area.is_none() {
            pr_err!("Direct map area is not available.");
            return Err(MemoryError::AllocAddressFailed);
        }
        match self
            .direct_mapped_area
            .as_mut()
            .unwrap()
            .allocator
            .alloc(size, MOrder::new(PAGE_SHIFT))
        {
            Ok(a) => Ok(a),
            Err(e) => {
                pr_err!("Failed to alloc direct mapped memory: {:?}", e);
                Err(MemoryError::AllocAddressFailed)
            }
        }
    }

    fn map_memory_from_direct_map(
        &mut self,
        address: PAddress,
        size: MSize,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        let mut entry = VirtualMemoryEntry::new(
            address.to_direct_mapped_v_address(),
            size.to_end_address(address.to_direct_mapped_v_address()),
            MemoryPermissionFlags::data(),
            MemoryOptionFlags::DIRECT_MAP | MemoryOptionFlags::DO_NOT_FREE_PHYSICAL_ADDRESS,
        );
        /* VirtualMemoryEntry has mutex lock inside. */
        if let Err(e) = self._map_address(
            &mut entry,
            address,
            address.to_direct_mapped_v_address(),
            size,
            pm_manager,
        ) {
            if let Err(e) = self
                .direct_mapped_area
                .as_mut()
                .unwrap()
                .allocator
                .free(address, size, false)
            {
                pr_err!("Failed to free direct mapped area: {:?}", e);
            }
            return Err(e);
        }

        if let Err(e) = self.insert_vm_map_entry(entry, pm_manager) {
            if let Err(e) = self
                .direct_mapped_area
                .as_mut()
                .unwrap()
                .allocator
                .free(address, size, false)
            {
                pr_err!("Failed to free direct mapped area: {:?}", e);
            }

            return Err(e);
        }

        /* already associated address */
        return Ok(());
    }

    pub(super) fn add_physical_memory_manager_pool(
        &mut self,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        let address = self.alloc_address_from_direct_map(PAGE_SIZE)?;
        pm_manager.add_memory_entry_pool(address.to_usize(), PAGE_SIZE_USIZE);
        self.map_memory_from_direct_map(address, PAGE_SIZE, pm_manager)?;
        return Ok(());
    }

    pub fn alloc_from_direct_map(
        &mut self,
        size: MSize,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<VAddress, MemoryError> {
        let address = self.alloc_address_from_direct_map(size)?;
        self.map_memory_from_direct_map(address, size, pm_manager)?;
        self.check_object_pools(pm_manager)?;
        return Ok(address.to_direct_mapped_v_address());
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

    #[allow(dead_code)]
    fn check_usable_address_range(
        &self,
        vm_start_address: VAddress,
        vm_end_address: VAddress,
    ) -> bool {
        assert!(vm_start_address < vm_end_address);
        let memory_area = vm_start_address..=vm_end_address;
        let direct_map_area = self.direct_mapped_area.as_ref().unwrap().start_address
            ..=self.direct_mapped_area.as_ref().unwrap().end_address;

        if Self::is_overlapped(&memory_area, &direct_map_area) {
            return false;
        }
        for e in unsafe { self.vm_entry.iter(offset_of!(VirtualMemoryEntry, list)) } {
            if (e.get_vm_start_address() <= vm_start_address
                && e.get_vm_end_address() >= vm_start_address)
                || (e.get_vm_start_address() <= vm_end_address
                    && e.get_vm_end_address() >= vm_end_address)
            {
                return false;
            }
            if e.get_vm_start_address() > vm_end_address {
                return true;
            }
        }
        true
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
                &((available_start_address)..=(end_address)),
                &(e.get_vm_start_address()..=e.get_vm_end_address()),
            ) {
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
