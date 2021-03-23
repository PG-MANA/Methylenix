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

use self::virtual_memory_entry::VirtualMemoryEntry;
/*use self::virtual_memory_object::VirtualMemoryObject;*/
use self::virtual_memory_page::VirtualMemoryPage;

use super::data_type::{
    Address, MIndex, MOrder, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress, VAddress,
};
use super::object_allocator::cache_allocator::CacheAllocator;
use super::physical_memory_manager::PhysicalMemoryManager;
use super::pool_allocator::PoolAllocator;
use super::MemoryError;

use crate::arch::target_arch::paging::{
    PageManager, PagingError, MAX_VIRTUAL_ADDRESS, PAGE_MASK, PAGE_SHIFT, PAGE_SIZE,
    PAGE_SIZE_USIZE, PAGING_CACHE_LENGTH,
};

use crate::kernel::collections::ptr_linked_list::PtrLinkedList;

use core::ops::RangeInclusive;

pub struct VirtualMemoryManager {
    vm_map_entry: PtrLinkedList<VirtualMemoryEntry>,
    is_system_vm: bool,
    page_manager: PageManager,
    vm_map_entry_pool: CacheAllocator<VirtualMemoryEntry>,
    /*vm_object_pool: PoolAllocator<VirtualMemoryObject>,*/
    vm_page_pool: CacheAllocator<VirtualMemoryPage>,
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
            vm_map_entry: PtrLinkedList::new(),
            is_system_vm: false,
            page_manager: PageManager::new(),
            vm_map_entry_pool: CacheAllocator::new(0),
            /*vm_object_pool: PoolAllocator::new(),*/
            vm_page_pool: CacheAllocator::new(0),
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
                MemoryOptionFlags::WIRED,
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
            self.associate_address(
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
        let alloc_func = |size: MSize, name: &str, p: &mut PhysicalMemoryManager| -> VAddress {
            if let Some(address) = p.alloc(size, MOrder::new(PAGE_SHIFT)) {
                address.to_direct_mapped_v_address()
            } else {
                panic!("Cannot alloc memory for {}.", name);
            }
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
                MemoryOptionFlags::NORMAL,
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
                if let Err(e) = vm_manager.associate_address(
                    address.to_direct_mapped_p_address() + i.to_offset(),
                    address + i.to_offset(),
                    MemoryPermissionFlags::data(),
                    p,
                ) {
                    panic!("Cannot associate address for {} Err:{:?}", name, e);
                }
            }
        };

        let vm_map_entry_pool_address =
            alloc_func(Self::VM_MAP_ENTRY_POOL_SIZE, "vm_map_entry", pm_manager);
        /*let vm_object_pool_address = alloc_func(Self::VM_OBJECT_POOL_SIZE, "vm_object", pm_manager);*/
        let vm_page_pool_address = alloc_func(Self::VM_PAGE_POOL_SIZE, "vm_page", pm_manager);

        self.vm_map_entry_pool
            .add_free_area(vm_map_entry_pool_address, Self::VM_MAP_ENTRY_POOL_SIZE);
        /*self.vm_object_pool
        .set_initial_pool(vm_object_pool_address, Self::VM_OBJECT_POOL_SIZE);*/
        self.vm_page_pool
            .add_free_area(vm_page_pool_address, Self::VM_PAGE_POOL_SIZE);

        map_func(
            self,
            "vm_map_entry",
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
            "{:#X} ~ {:#X} (size: {:#X}) are reserved for direct map",
            direct_mapped_area_address.to_usize(),
            direct_mapped_area_size
                .to_end_address(direct_mapped_area_address)
                .to_usize(),
            direct_mapped_area_size.to_usize()
        );

        self.associate_address_with_size(
            direct_mapped_area_address,
            direct_mapped_area_address.to_direct_mapped_v_address(),
            direct_mapped_area_size,
            MemoryPermissionFlags::data(),
            pm_manager,
        )
        .expect("Cannot associate address for direct map");

        let mut direct_mapped_area_allocator = PhysicalMemoryManager::new();
        direct_mapped_area_allocator
            .set_memory_entry_pool(direct_mapped_area_address.to_usize(), 2 << PAGE_SHIFT);
        direct_mapped_area_allocator.free(
            direct_mapped_area_address + MSize::new(2 << PAGE_SHIFT),
            direct_mapped_area_size - MSize::new(2 << PAGE_SHIFT),
            true,
        );

        self.map_memory_from_direct_map(
            direct_mapped_area_address,
            MSize::new(2 << PAGE_SHIFT),
            pm_manager,
        )
        .expect("Cannot insert vm_map_entry for direct mapped area allocator.");

        self.direct_mapped_area = Some(DirectMappedArea {
            allocator: direct_mapped_area_allocator,
            start_address: direct_mapped_area_address.to_direct_mapped_v_address(),
            end_address: direct_mapped_area_size
                .to_end_address(direct_mapped_area_address)
                .to_direct_mapped_v_address(),
        });
    }

    pub fn flush_paging(&mut self) {
        self.page_manager.flush_page_table();
    }

    pub fn update_paging(&mut self /*Not necessary*/, address: VAddress) {
        PageManager::update_page_cache(address);
    }

    /* alloc virtual address for "physical_address" and map linearly.
    if map non-linearly, use alloc_non_linear_address() */
    pub fn alloc_address(
        &mut self,
        size: MSize,
        physical_address: PAddress,
        permission: MemoryPermissionFlags,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<VAddress, MemoryError> {
        /* NOTE: ページキャッシュの更新は行わない */
        if physical_address & !PAGE_MASK != 0 {
            pr_err!("Physical Address is not aligned.");
            return Err(MemoryError::AddressNotAligned);
        } else if size & !PAGE_MASK != 0 {
            pr_err!("Size is not aligned.");
            return Err(MemoryError::SizeNotAligned);
        }
        let vm_start_address = if self.check_usable_address_range(
            physical_address.to_direct_mapped_v_address(),
            size.to_end_address(physical_address)
                .to_direct_mapped_v_address(),
        ) {
            VAddress::new(physical_address.to_usize())
        } else if let Some(address) = self.find_usable_memory_area(size) {
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
            MemoryOptionFlags::NORMAL,
            pm_manager,
        )
    }

    /*不連続な物理メモリをマップする際に使う*/
    pub fn alloc_address_without_mapping(
        &mut self,
        size: MSize,
        permission: MemoryPermissionFlags,
        option: MemoryOptionFlags,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<&'static mut VirtualMemoryEntry, MemoryError> {
        if size & !PAGE_MASK != 0 {
            pr_err!("Size is not aligned.");
            return Err(MemoryError::SizeNotAligned);
        }
        let entry = if let Some(address) = self.find_usable_memory_area(size) {
            VirtualMemoryEntry::new(address, size.to_end_address(address), permission, option)
        } else {
            pr_warn!("Virtual Address is not available.");
            return Err(MemoryError::InvalidVirtualAddress);
        };

        self.insert_vm_map_entry(entry, pm_manager)
    }

    /*vm_map_entryはalloc_address_with_mappingで確保されたものでないといけない*/
    pub fn insert_physical_page_into_vm_map_entry(
        &mut self,
        vm_map_entry: &mut VirtualMemoryEntry,
        vm_address: VAddress,
        physical_address /*must be allocated*/: PAddress,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        if physical_address & !PAGE_MASK != 0 {
            pr_err!(
                "Physical Address is not aligned: {:#x}",
                physical_address.to_usize()
            );
            return Err(MemoryError::AddressNotAligned);
        } else if vm_address & !PAGE_MASK != 0 {
            pr_err!(
                "Virtual Address is not aligned: {:#x}",
                vm_address.to_usize()
            );
            return Err(MemoryError::AddressNotAligned);
        } else if vm_map_entry.get_vm_start_address() > vm_address
            || vm_map_entry.get_vm_end_address() < PAGE_SIZE.to_end_address(vm_address)
        {
            pr_err!("Virtual Address is out of vm_map_entry.");
            return Err(MemoryError::InvalidVirtualAddress);
        }

        self._map_address(
            vm_map_entry,
            physical_address,
            vm_address,
            PAGE_SIZE,
            pm_manager,
        )
    }

    /*Don't use vm_map_entry from alloc_address_without_mapping after call this function */
    pub fn finalize_vm_map_entry(
        &mut self,
        vm_map_entry: &'static mut VirtualMemoryEntry,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        vm_map_entry.get_object_mut().activate_all_page();
        let first_p_index = vm_map_entry.get_memory_offset().to_index();
        let last_p_index = MIndex::from_offset(
            vm_map_entry.get_vm_end_address() - vm_map_entry.get_vm_start_address()
                + vm_map_entry.get_memory_offset(), /* Is it ok? */
        ) + MIndex::new(1);
        for i in first_p_index..last_p_index {
            if let Some(p) = vm_map_entry.get_object().get_vm_page(i) {
                if self
                    .associate_address(
                        p.get_physical_address(),
                        vm_map_entry.get_vm_start_address() + i.to_offset(), /*is it ok?*/
                        vm_map_entry.get_permission_flags(),
                        pm_manager,
                    )
                    .is_err()
                {
                    panic!("Cannot associate address (TODO: disassociation)");
                }
            }
        }
        self.check_object_pools(pm_manager)?;
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
        if physical_address & !PAGE_MASK != 0 {
            return Err(MemoryError::AddressNotAligned);
        } else if size & !PAGE_MASK != 0 {
            return Err(MemoryError::SizeNotAligned);
        }
        let mut entry = if let Some(vm_start_address) = virtual_address {
            if vm_start_address.to_usize() & !PAGE_MASK != 0 {
                return Err(MemoryError::AddressNotAligned);
            }
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
        } else if let Some(vm_start_address) = self.find_usable_memory_area(size) {
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

        if entry.get_memory_option_flags().is_dev_map() {
            if self
                .associate_address_with_size(
                    physical_address,
                    vm_start_address,
                    size,
                    permission,
                    pm_manager,
                )
                .is_err()
            {
                pr_err!("Cannot associate address.");
                self.vm_map_entry.remove(&mut entry.list);
                //for rev_i in (0..i).rev() {
                /* ADD: self.unassociate_address() */
                //}
                self.vm_map_entry_pool.free(entry);
                return Err(MemoryError::PagingError);
            }
            for i in MIndex::new(0)..size.to_index() {
                self.update_paging(vm_start_address + i.to_offset());
            }
        } else {
            for i in MIndex::new(0)..size.to_index() {
                if self
                    .associate_address(
                        physical_address + i.to_offset(),
                        vm_start_address + i.to_offset(),
                        permission,
                        pm_manager,
                    )
                    .is_err()
                {
                    pr_err!("Cannot associate address.");
                    self.vm_map_entry.remove(&mut entry.list);
                    //for rev_i in (0..i).rev() {
                    /* ADD: self.unassociate_address() */
                    //}
                    self.vm_map_entry_pool.free(entry);
                    return Err(MemoryError::PagingError);
                }
                self.update_paging(vm_start_address + i.to_offset());
            }
        }
        self.check_object_pools(pm_manager)?;
        Ok(vm_start_address)
    }

    pub fn mmap_dev(
        &mut self,
        physical_address: PAddress,
        virtual_address: Option<VAddress>,
        size: MSize,
        permission: MemoryPermissionFlags,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<VAddress, MemoryError> {
        assert_eq!(permission.is_executable(), false); /* Disallow executing code on device mapping */
        self.map_address(
            physical_address,
            virtual_address,
            size,
            permission,
            MemoryOptionFlags::DEV_MAP,
            pm_manager,
        )
    }

    pub fn free_address(
        &mut self,
        vm_start_address: VAddress,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        if vm_start_address & !PAGE_MASK != 0 {
            pr_err!("Virtual Address is not aligned.");
            return Err(MemoryError::AddressNotAligned);
        }
        if self.vm_map_entry.is_empty() {
            pr_err!("There is no entry.");
            return Err(MemoryError::InsertEntryFailed);
        }
        if let Some(vm_map_entry) = self.find_entry_mut(vm_start_address) {
            self._free_address(vm_map_entry, pm_manager)
        } else {
            pr_err!("Cannot find vm_map_entry.");
            Err(MemoryError::InvalidVirtualAddress)
        }
    }

    fn _free_address(
        &mut self,
        vm_map_entry /* will be removed from list and freed */: &'static mut VirtualMemoryEntry,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        let first_p_index = vm_map_entry.get_memory_offset().to_index();
        let last_p_index = MIndex::from_offset(
            MSize::from_address(
                vm_map_entry.get_vm_start_address(),
                vm_map_entry.get_vm_end_address(),
            ) + vm_map_entry.get_memory_offset(), /* Is it ok? */
        ) + MIndex::new(1);
        for i in first_p_index..last_p_index {
            if let Some(p) = vm_map_entry.get_object_mut().remove_vm_page(i) {
                if self
                    .unassociate_address(
                        vm_map_entry.get_vm_start_address() + i.to_offset(), /*is it ok?*/
                        pm_manager,
                    )
                    .is_err()
                {
                    panic!("Cannot unassociate address.");
                }
                if !vm_map_entry
                    .get_memory_option_flags()
                    .should_not_free_phy_address()
                {
                    pm_manager.free(p.get_physical_address(), PAGE_SIZE, false);
                }
                self.vm_page_pool.free(p);
            }
        }
        if vm_map_entry.get_memory_option_flags().is_direct_mapped()
            && !self.direct_mapped_area.as_mut().unwrap().allocator.free(
                vm_map_entry
                    .get_vm_start_address()
                    .to_direct_mapped_p_address(),
                vm_map_entry.get_size(),
                false,
            )
        {
            pr_err!("Cannot free direct mapped area");
        }

        self.vm_map_entry.remove(&mut vm_map_entry.list);
        self.adjust_vm_entries();
        vm_map_entry.set_disabled();
        self.vm_map_entry_pool.free(vm_map_entry);

        return Ok(());
    }

    fn insert_vm_map_entry(
        &mut self,
        source: VirtualMemoryEntry,
        _pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<&'static mut VirtualMemoryEntry, MemoryError> {
        let entry = match self.vm_map_entry_pool.alloc(None) {
            Ok(e) => e,
            Err(_) => {
                self.add_vm_map_entry_pool(None)?;
                self.vm_map_entry_pool.alloc(None)?
            }
        };
        *entry = source;
        if self.vm_map_entry.is_empty() {
            self.vm_map_entry.insert_head(&mut entry.list);
        } else if let Some(prev_entry) = self.find_previous_entry_mut(entry.get_vm_start_address())
        {
            self.vm_map_entry
                .insert_after(&mut prev_entry.list, &mut entry.list);
        } else if entry.get_vm_end_address()
            < unsafe {
                self.vm_map_entry
                    .get_first_entry(offset_of!(VirtualMemoryEntry, list))
            }
            .unwrap()
            .get_vm_start_address()
        {
            self.vm_map_entry.insert_head(&mut entry.list);
        } else {
            pr_err!("Cannot insert Virtual Memory Entry.");
            return Err(MemoryError::InsertEntryFailed);
        }
        self.adjust_vm_entries();
        return Ok(entry);
    }

    /// insert pages into entry (not sync with PageManager)
    /// virtual_address must be allocated.
    fn _map_address(
        &mut self,
        vm_map_entry: &mut VirtualMemoryEntry,
        physical_address: PAddress,
        virtual_address /* must allocated */: VAddress,
        size: MSize,
        _pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        if physical_address & !PAGE_MASK != 0 {
            pr_err!(
                "Physical Address is not aligned: {:#x}",
                physical_address.to_usize()
            );
            return Err(MemoryError::AddressNotAligned);
        } else if virtual_address & !PAGE_MASK != 0 {
            pr_err!(
                "Virtual Address is not aligned: {:#x}",
                virtual_address.to_usize()
            );
            return Err(MemoryError::AddressNotAligned);
        } else if size & !PAGE_MASK != 0 {
            pr_err!("Size is not aligned: {:#x}", size.to_usize());
            return Err(MemoryError::SizeNotAligned);
        } else if size.is_zero() {
            pr_err!("Size is zero");
            return Err(MemoryError::InvalidSize);
        }
        for i in MIndex::new(0)..size.to_index() {
            self.insert_page_into_vm_map_entry(
                vm_map_entry,
                virtual_address + i.to_offset(),
                physical_address + i.to_offset(),
                _pm_manager,
            )?;
        }
        Ok(())
    }

    fn insert_page_into_vm_map_entry(
        &mut self,
        vm_map_entry: &mut VirtualMemoryEntry,
        virtual_address: VAddress,
        physical_address: PAddress,
        _pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        assert!(vm_map_entry.get_vm_start_address() <= virtual_address);
        assert!(vm_map_entry.get_vm_end_address() >= PAGE_SIZE.to_end_address(virtual_address));
        let p_index = MIndex::from_offset(virtual_address - vm_map_entry.get_vm_start_address());
        let vm_page = match self.vm_page_pool.alloc(None) {
            Ok(p) => p,
            Err(_) => {
                self.add_vm_page_pool(None)?;
                self.vm_page_pool.alloc(None)?
            }
        };
        *vm_page = VirtualMemoryPage::new(physical_address, p_index);
        vm_page.set_page_status(vm_map_entry.get_memory_option_flags());
        vm_page.activate();
        vm_map_entry.get_object_mut().add_vm_page(p_index, vm_page);
        Ok(())
    }

    fn associate_address(
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
                            Err(e) => panic!("Cannot alloc memory for paging Err:{:?}", e),
                        }
                    }
                    /* retry (by loop) */
                }
                Err(_) => {
                    pr_err!("Cannot associate physical address.");
                    return Err(MemoryError::PagingError);
                }
            };
        }
    }

    fn associate_address_with_size(
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
                            Err(e) => panic!("Cannot alloc memory for paging Err:{:?}", e),
                        }
                    }
                    /* retry (by loop) */
                }
                Err(_) => {
                    pr_err!("Cannot associate physical address.");
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

    fn try_expand_size(
        &mut self,
        target_entry: &mut VirtualMemoryEntry,
        new_size: MSize,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> bool {
        if MSize::from_address(
            target_entry.get_vm_start_address(),
            target_entry.get_vm_end_address(),
        ) >= new_size
        {
            return true;
        }
        if let Some(next_entry) = unsafe {
            target_entry
                .list
                .get_next(offset_of!(VirtualMemoryEntry, list))
        } {
            let next_entry_start_address =
                unsafe { &*(next_entry as *const VirtualMemoryEntry) }.get_vm_start_address();
            if new_size.to_end_address(target_entry.get_vm_start_address())
                >= next_entry_start_address
            {
                return false;
            }
        } else if new_size.to_end_address(target_entry.get_vm_start_address())
            >= MAX_VIRTUAL_ADDRESS
        {
            return false;
        }

        let old_size = MSize::from_address(
            target_entry.get_vm_start_address(),
            target_entry.get_vm_end_address(),
        );
        let old_last_p_index = MIndex::from_offset(
            target_entry.get_vm_end_address() - target_entry.get_vm_start_address()
                + target_entry.get_memory_offset(), /* Is it ok? */
        );
        let not_associated_virtual_address = target_entry.get_vm_end_address() + MSize::new(1);
        let not_associated_physical_address = target_entry
            .get_object()
            .get_vm_page(old_last_p_index)
            .unwrap()
            .get_physical_address()
            + PAGE_SIZE;

        target_entry
            .set_vm_end_address(new_size.to_end_address(target_entry.get_vm_start_address()));

        for i in MIndex::new(0)..MIndex::from_offset(new_size - old_size) {
            if let Err(s) = self._map_address(
                target_entry,
                not_associated_physical_address + i.to_offset(),
                not_associated_virtual_address + i.to_offset(),
                PAGE_SIZE,
                pm_manager,
            ) {
                pr_err!("{:?}", s);
                panic!("Cannot insert vm_page");
            }
        }
        target_entry.get_object_mut().activate_all_page();
        for i in MIndex::new(0)..MIndex::from_offset(new_size - old_size) {
            if self
                .associate_address(
                    not_associated_physical_address + i.to_offset(),
                    not_associated_virtual_address + i.to_offset(),
                    target_entry.get_permission_flags(),
                    pm_manager,
                )
                .is_err()
            {
                if !i.is_zero() {
                    target_entry.set_vm_end_address(
                        not_associated_virtual_address + (i - MIndex::new(1)).to_offset(),
                    );
                }
                return false;
            }
        }
        return true;
    }

    pub fn resize_memory_mapping(
        &mut self,
        virtual_address: VAddress,
        new_size: MSize,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<VAddress, MemoryError> {
        if virtual_address & !PAGE_MASK != 0 {
            pr_err!(
                "Virtual Address is not aligned: {:#x}",
                virtual_address.to_usize()
            );
            return Err(MemoryError::AddressNotAligned);
        } else if new_size & !PAGE_MASK != 0 {
            pr_err!("Size is not aligned: {:#x}", new_size.to_usize());
            return Err(MemoryError::SizeNotAligned);
        } else if new_size.is_zero() {
            pr_err!("Size is zero");
            return Err(MemoryError::InvalidSize);
        } else if self.vm_map_entry.is_empty() {
            pr_err!("There is no entry.");
            return Err(MemoryError::InsertEntryFailed); /* Is it ok? */
        }
        if let Some(entry) = self.find_entry_mut(virtual_address) {
            if !entry.get_memory_option_flags().is_dev_map() {
                pr_err!("Not dev_mapped entry.");
                return Err(MemoryError::InvalidVirtualAddress);
            }
            if self.try_expand_size(entry, new_size, pm_manager) {
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
            Err(MemoryError::InvalidVirtualAddress)
        }
    }

    fn add_vm_map_entry_pool(
        &mut self,
        mut pm_manager: Option<&mut PhysicalMemoryManager>,
    ) -> Result<(), MemoryError> {
        if self.vm_map_entry_pool.len() != 0 && pm_manager.is_some() {
            let pm_manager = pm_manager.as_mut().unwrap();
            if let Some(p_address) =
                pm_manager.alloc(Self::VM_MAP_ENTRY_POOL_SIZE, PAGE_SHIFT.into())
            {
                match self.alloc_address(
                    Self::VM_MAP_ENTRY_POOL_SIZE,
                    p_address,
                    MemoryPermissionFlags::data(),
                    pm_manager,
                ) {
                    Ok(v_address) => {
                        self.update_paging(v_address);
                        self.vm_map_entry_pool
                            .add_free_area(v_address, Self::VM_MAP_ENTRY_POOL_SIZE);
                        return Ok(());
                    }
                    Err(e) => {
                        pr_err!(
                            "Cannot Allocate Virtual Address for vm_map_entry. Error: {:?}",
                            e
                        );
                        pm_manager.free(p_address, Self::VM_MAP_ENTRY_POOL_SIZE, false);
                    }
                }
            }
        }
        /* Allocate from Direct Mapped Area */
        let address = self.alloc_address_from_direct_map(PAGE_SIZE)?;
        self.vm_map_entry_pool
            .add_free_area(address.to_direct_mapped_v_address(), PAGE_SIZE);
        self.map_memory_from_direct_map(
            address,
            PAGE_SIZE,
            pm_manager.unwrap_or(&mut PhysicalMemoryManager::new() /* Temporary*/),
        )?;
        pr_info!("Allocated vm_map_entry pool from direct mapped area.");
        return Ok(());
    }

    fn add_vm_page_pool(
        &mut self,
        mut pm_manager: Option<&mut PhysicalMemoryManager>,
    ) -> Result<(), MemoryError> {
        if self.vm_page_pool.len() != 0 && pm_manager.is_some() {
            let pm_manager = pm_manager.as_mut().unwrap();
            let alloc_size = PAGE_SIZE * MSize::new(self.vm_page_pool.len() - 1);
            if let Some(p_address) = pm_manager.alloc(alloc_size, PAGE_SHIFT.into()) {
                match self.alloc_address(
                    alloc_size,
                    p_address,
                    MemoryPermissionFlags::data(),
                    pm_manager,
                ) {
                    Ok(v_address) => {
                        self.update_paging(v_address);
                        self.vm_page_pool.add_free_area(v_address, alloc_size);
                        return Ok(());
                    }
                    Err(e) => {
                        pr_err!(
                            "Cannot Allocate Virtual Address for vm_page. Error: {:?}",
                            e
                        );
                        pm_manager.free(p_address, alloc_size, false);
                    }
                }
            }
        }
        /* Allocate from Direct Mapped Area */
        let p_address = self.alloc_address_from_direct_map(PAGE_SIZE)?;
        self.vm_page_pool
            .add_free_area(p_address.to_direct_mapped_v_address(), PAGE_SIZE);
        self.map_memory_from_direct_map(
            p_address,
            PAGE_SIZE,
            pm_manager.unwrap_or(&mut PhysicalMemoryManager::new() /*Temporary*/),
        )?;
        pr_info!("Allocated vm_page pool from direct mapped area.");
        return Ok(());
    }

    fn check_object_pools(
        &mut self,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        if self.vm_page_pool.len() < Self::VM_PAGE_CACHE_LEN {
            self.add_vm_page_pool(Some(pm_manager))?;
        }
        if self.vm_map_entry_pool.len() < Self::VM_MAP_ENTRY_CACHE_LEN {
            self.add_vm_map_entry_pool(Some(pm_manager))?;
        }
        return Ok(());
    }

    fn alloc_address_from_direct_map(&mut self, size: MSize) -> Result<PAddress, MemoryError> {
        if size & !PAGE_MASK != 0 {
            pr_err!("Size is not aligned: {:#x}", size.to_usize());
            return Err(MemoryError::SizeNotAligned);
        }
        if self.direct_mapped_area.is_none() {
            pr_err!("Direct map area is not available.");
            return Err(MemoryError::AllocPhysicalAddressFailed);
        }
        match self
            .direct_mapped_area
            .as_mut()
            .unwrap()
            .allocator
            .alloc(size, MOrder::new(PAGE_SHIFT))
        {
            Some(a) => Ok(a),
            None => {
                pr_err!("Cannot alloc from direct map.");
                Err(MemoryError::AllocPhysicalAddressFailed)
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
            self.direct_mapped_area
                .as_mut()
                .unwrap()
                .allocator
                .free(address, size, false);
            return Err(e);
        }

        if let Err(e) = self.insert_vm_map_entry(entry, pm_manager) {
            self.direct_mapped_area
                .as_mut()
                .unwrap()
                .allocator
                .free(address, size, false);
            return Err(e);
        }

        /* already associated address */
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
        for e in unsafe { self.vm_map_entry.iter(offset_of!(VirtualMemoryEntry, list)) } {
            if e.get_vm_start_address() <= vm_address && e.get_vm_end_address() >= vm_address {
                return Some(e);
            }
        }
        None
    }

    fn find_entry_mut(&mut self, vm_address: VAddress) -> Option<&'static mut VirtualMemoryEntry> {
        for e in unsafe {
            self.vm_map_entry
                .iter_mut(offset_of!(VirtualMemoryEntry, list))
        } {
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
        for e in unsafe { self.vm_map_entry.iter_mut(OFFSET) } {
            if e.get_vm_start_address() > vm_address {
                return unsafe { e.list.get_prev_mut(OFFSET) };
            } else if !e.list.has_next() && e.get_vm_end_address() < vm_address {
                return Some(e);
            }
        }
        None
    }

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
        for e in unsafe { self.vm_map_entry.iter(offset_of!(VirtualMemoryEntry, list)) } {
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

    pub fn find_usable_memory_area(&self, size: MSize) -> Option<VAddress> {
        let direct_map_start_address = self.direct_mapped_area.as_ref().unwrap().start_address;
        let direct_map_end_address = self.direct_mapped_area.as_ref().unwrap().end_address;
        let direct_map_area = direct_map_start_address..=direct_map_end_address;
        const OFFSET: usize = offset_of!(VirtualMemoryEntry, list);
        for e in unsafe { self.vm_map_entry.iter(OFFSET) } {
            if let Some(prev) = unsafe { e.list.get_prev(OFFSET) } {
                if e.get_vm_start_address() - (prev.get_vm_end_address() + MSize::new(1)) >= size {
                    let start_address = prev.get_vm_end_address() + MSize::new(1);
                    let memory_area = start_address..=size.to_end_address(start_address);

                    if Self::is_overlapped(&direct_map_area, &memory_area) {
                        if e.get_vm_start_address() - (direct_map_end_address + MSize::new(1))
                            >= size
                        {
                            return Some(direct_map_end_address + MSize::new(1));
                        } else {
                            continue;
                        }
                    }
                    return Some(prev.get_vm_end_address() + MSize::new(1));
                }
            }
            if !e.list.has_next() {
                return if e.get_vm_end_address() + MSize::new(1) + size >= MAX_VIRTUAL_ADDRESS {
                    None
                } else {
                    let start_address = e.get_vm_end_address() + MSize::new(1);
                    let end_address = size.to_end_address(start_address);
                    let memory_area = start_address..=end_address;

                    if Self::is_overlapped(&direct_map_area, &memory_area) {
                        return if (direct_map_end_address + MSize::new(1) + size)
                            >= MAX_VIRTUAL_ADDRESS
                        {
                            None
                        } else {
                            Some(direct_map_end_address + MSize::new(1))
                        };
                    }
                    Some(e.get_vm_end_address() + MSize::new(1))
                };
            }
        }
        unreachable!()
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
        if self.vm_map_entry.is_empty() {
            kprintln!("There is no root entry.");
            return;
        }
        let offset = offset_of!(VirtualMemoryEntry, list);
        let mut entry = unsafe { self.vm_map_entry.get_first_entry(offset) }.unwrap();
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
                "Virtual Address:{:#X} Size:{:#X} W:{}, U:{}, EXE:{}",
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
                            "...\n - {} Physical Address:{:#X}",
                            i.to_usize() - 1,
                            last_address.to_usize()
                        );
                        omitted = false;
                    }
                    kprintln!(
                        " - {} Physical Address:{:#X}",
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
                    "...\n - {} Physical Address:{:#X} (fin)",
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
