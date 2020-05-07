/*
 * Virtual Memory Manager
 * This manager maintains memory map and controls page_manager.
 * The address and size are rounded up to an integral number of PAGE_SIZE.
*/

/* add: add physical_memory into reserved_memory_list when it runs out */

use arch::target_arch::paging::{PageManager, PagingError};
use arch::target_arch::paging::{MAX_VIRTUAL_ADDRESS, PAGE_MASK, PAGE_SIZE, PAGING_CACHE_LENGTH};

use kernel::memory_manager::physical_memory_manager::PhysicalMemoryManager;
use kernel::memory_manager::virtual_memory_entry::VirtualMemoryEntry;
use kernel::memory_manager::{FreePageList, MemoryOptionFlags, MemoryPermissionFlags};

pub struct VirtualMemoryManager {
    vm_map_entry: usize,
    is_system_vm: bool,
    page_manager: PageManager,
    entry_pool: usize,
    entry_pool_size: usize,
    reserved_memory_list: FreePageList,
}

impl VirtualMemoryManager {
    pub const fn new() -> Self {
        Self {
            vm_map_entry: 0,
            is_system_vm: false,
            page_manager: PageManager::new(),
            entry_pool: 0,
            entry_pool_size: 0,
            reserved_memory_list: FreePageList {
                list: [0; PAGING_CACHE_LENGTH],
                pointer: 0,
            },
        }
    }

    pub fn init(&mut self, is_system_vm: bool, pm_manager: &mut PhysicalMemoryManager) -> bool {
        //MEMO: 勝手にダイレクトマッピング?(ストレートマッピング?)をしているが、
        //      システム起動時・プロセス起動時にダイレクトマッピングされており且つ
        //      PhysicalMemoryManagerにおいて先に利用されているメモリ領域が予約されていることに依存している。

        self.is_system_vm = is_system_vm;

        /* set up cache list */
        for i in 0..PAGING_CACHE_LENGTH {
            let cache_address = pm_manager
                .alloc(PAGE_SIZE, true)
                .expect("Cannot alloc memory for paging cache");
            self.reserved_memory_list.list[i] = cache_address;
        }
        self.reserved_memory_list.pointer = PAGING_CACHE_LENGTH;

        /* set up page_manager */
        if !self.page_manager.init(&mut self.reserved_memory_list) {
            return false;
        }

        /* set up memory pool */
        self.entry_pool_size = PAGE_SIZE * 4;
        self.entry_pool = if let Some(address) = pm_manager.alloc(self.entry_pool_size, true) {
            address
        } else {
            self.entry_pool_size = 0;
            return false;
        };
        for i in 0..(self.entry_pool_size / VirtualMemoryEntry::ENTRY_SIZE) {
            unsafe {
                (*((self.entry_pool + i * VirtualMemoryEntry::ENTRY_SIZE)
                    as *mut VirtualMemoryEntry))
                    .set_disabled()
            }
        }
        self.vm_map_entry = self.entry_pool;

        for i in 0..(self.entry_pool_size / PAGE_SIZE) {
            if !self.associate_address(
                self.entry_pool + i * PAGE_SIZE,
                self.entry_pool + i * PAGE_SIZE,
                MemoryPermissionFlags::data(),
                pm_manager,
            ) {
                panic!("Cannot associate address for virtual memory manager's pool");
            }
        }
        unsafe {
            *(self.entry_pool as *mut VirtualMemoryEntry) = VirtualMemoryEntry::new(
                self.entry_pool,
                self.entry_pool + self.entry_pool_size - 1,
                self.entry_pool,
                MemoryPermissionFlags::data(),
                MemoryOptionFlags::new(MemoryOptionFlags::NORMAL),
            );
        }

        /* insert cached_memory_list entry */
        for i in 0..PAGING_CACHE_LENGTH
        /* 既に使われた分も */
        {
            let cache_address = self.reserved_memory_list.list[i];
            self.insert_entry(
                VirtualMemoryEntry::new(
                    cache_address,
                    PhysicalMemoryManager::size_to_end_address(cache_address, PAGE_SIZE),
                    cache_address,
                    MemoryPermissionFlags::data(),
                    MemoryOptionFlags::new(MemoryOptionFlags::WIRED),
                ),
                pm_manager,
            );
            if !self.associate_address(
                cache_address,
                cache_address,
                MemoryPermissionFlags::data(),
                pm_manager,
            ) {
                panic!("Cannot associate address for paging cache");
            }
        }
        true
    }

    pub fn flush_paging(&mut self) {
        self.page_manager.reset_paging();
    }

    pub fn update_paging(&mut self /*Not necessary*/, address: usize) {
        PageManager::reset_paging_local(address);
    }

    fn insert_entry(
        &mut self,
        entry: VirtualMemoryEntry,
        _pm_manager: &mut PhysicalMemoryManager,
    ) -> Option<&'static mut VirtualMemoryEntry> {
        for i in 0..(self.entry_pool_size / VirtualMemoryEntry::ENTRY_SIZE) {
            let e = unsafe {
                &mut (*((self.entry_pool + i * VirtualMemoryEntry::ENTRY_SIZE)
                    as *mut VirtualMemoryEntry))
            };
            if e.is_disabled() {
                *e = entry;
                if !(unsafe { &mut (*(self.vm_map_entry as *mut VirtualMemoryEntry)) }
                    .insert_entry(e))
                {
                    pr_err!("Cannot insert Virtual Memory Entry.");
                    return None;
                }
                self.vm_map_entry =
                    unsafe { &mut (*(self.vm_map_entry as *mut VirtualMemoryEntry)) }
                        .adjust_entries();
                return Some(e);
            }
        }
        //TODO: realloc entry
        pr_err!("WoW");
        self.dump_memory_manager();
        return None;
    }

    pub fn alloc_address(
        &mut self,
        size: usize,
        physical_start_address: usize,
        permission: MemoryPermissionFlags,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Option<usize> {
        /* thinking map_memory */
        //NOTE: ページキャッシュの更新は行わない
        if physical_start_address & !PAGE_MASK != 0 {
            pr_err!("Physical Address is not aligned.");
            return None;
        } else if size & !PAGE_MASK != 0 {
            pr_err!("Size is not aligned.");
            return None;
        }
        let entry = if self.check_if_usable_address_range(
            physical_start_address,
            PhysicalMemoryManager::size_to_end_address(physical_start_address, size),
        ) {
            /* 物理・論理アドレスの合致ができるならそれが良い */
            VirtualMemoryEntry::new(
                physical_start_address,
                PhysicalMemoryManager::size_to_end_address(physical_start_address, size),
                physical_start_address,
                permission,
                MemoryOptionFlags::new(MemoryOptionFlags::NORMAL), /* temporary */
            )
        } else if let Some(address) = self.get_free_address(size) {
            VirtualMemoryEntry::new(
                address,
                PhysicalMemoryManager::size_to_end_address(address, size),
                physical_start_address,
                permission,
                MemoryOptionFlags::new(MemoryOptionFlags::NORMAL),
            )
        } else {
            pr_warn!("Virtual Address is not available.");
            return None;
        };
        let vm_start_address = entry.get_vm_start_address();
        let entry = self.insert_entry(entry, pm_manager);
        if entry.is_none() {
            pr_warn!("Cannot add Virtual Memory Entry.");
            return None;
        }
        let entry = entry.unwrap();
        for i in 0..size / PAGE_SIZE {
            if !self.associate_address(
                physical_start_address + i * PAGE_SIZE,
                vm_start_address + i * PAGE_SIZE,
                permission,
                pm_manager,
            ) {
                entry.set_vm_end_address(PhysicalMemoryManager::size_to_end_address(
                    entry.get_vm_start_address(),
                    (i - 1) * PAGE_SIZE,
                ));
                self.free_address(entry.get_vm_start_address(), pm_manager);
                return None;
            }
        }
        Some(vm_start_address)
    }

    pub fn free_address(
        &mut self,
        vm_start_address: usize,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> bool {
        if vm_start_address & !PAGE_MASK != 0 {
            pr_info!("Virtual Address is not aligned.");
            return false;
        }
        let mut cloned_root_entry =
            unsafe { &mut *(self.vm_map_entry as *mut VirtualMemoryEntry) }.clone(); /* avoid compile error by borrow checker */
        let mut root_entry = unsafe { &mut *(self.vm_map_entry as *mut VirtualMemoryEntry) };
        let mut root_physical_address = root_entry.get_physical_address();
        let entry = root_entry.find_entry_mut(vm_start_address);
        if entry.is_none() {
            return false;
        }
        let entry = entry.unwrap();
        if entry.get_physical_address() == root_physical_address {
            if let Some(next) = cloned_root_entry.get_next_entry() {
                self.vm_map_entry = next;
                cloned_root_entry =
                    unsafe { &mut *(self.vm_map_entry as *mut VirtualMemoryEntry) }.clone(); /* avoid compile error by borrow checker */
                root_entry = unsafe { &mut *(self.vm_map_entry as *mut VirtualMemoryEntry) };
                root_physical_address = root_entry.get_physical_address();
            } else {
                panic!("No entry is available.");
            }
        }
        let vm_start_address = entry.get_vm_start_address();
        let size =
            PhysicalMemoryManager::address_to_size(vm_start_address, entry.get_vm_end_address());
        let physical_address = entry.get_physical_address();
        let option = entry.get_memory_option_flags();
        entry.delete_entry();
        unsafe { &mut *(self.vm_map_entry as *mut VirtualMemoryEntry) }.adjust_entries();
        let mut maybe_retry = false;
        for i in 0..(size / PAGE_SIZE) {
            loop {
                match self.page_manager.unassociate_address(
                    vm_start_address + i * PAGE_SIZE,
                    &mut self.reserved_memory_list,
                    maybe_retry,
                ) {
                    Ok(()) => {
                        maybe_retry = false;
                        break; /* break "loop" (not "for") */
                    }
                    Err(PagingError::MemoryCacheOverflowed) => {
                        for i in ((PAGING_CACHE_LENGTH / 2)..PAGING_CACHE_LENGTH).rev() {
                            if let Some(page_cache_entry) =
                                root_entry.find_entry_mut(self.reserved_memory_list.list[i])
                            {
                                if page_cache_entry.get_physical_address() == root_physical_address
                                {
                                    if let Some(next) = cloned_root_entry.get_next_entry() {
                                        self.vm_map_entry = next;
                                        root_entry = unsafe {
                                            &mut *(self.vm_map_entry as *mut VirtualMemoryEntry)
                                        };
                                        root_physical_address = root_entry.get_physical_address();
                                    } else {
                                        panic!("No entry is available.");
                                    }
                                }
                                page_cache_entry.delete_entry();
                                if !pm_manager.free(
                                    self.reserved_memory_list.list[i],
                                    PAGE_SIZE,
                                    false,
                                ) {
                                    panic!("Cannot free physical memory for paging cache");
                                }
                            } else {
                                panic!("Cannot find virtual memory entry for paging cache");
                            }
                        }
                        self.reserved_memory_list.pointer = PAGING_CACHE_LENGTH / 2;
                        maybe_retry = true;
                        /* retry (by loop) */
                    }
                    Err(_) => {
                        pr_err!("Cannot unassociate memory");
                        return false;
                    }
                };
            }
        }
        if !option.do_not_free_phy_addr() {
            pm_manager.free(physical_address, size, false); //should do this at MemoryManager...?
        }
        true
    }

    fn associate_address(
        &mut self,
        phsyical_address: usize,
        virtual_address: usize,
        permission: MemoryPermissionFlags,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> bool {
        loop {
            match self.page_manager.associate_address(
                &mut self.reserved_memory_list,
                phsyical_address,
                virtual_address,
                permission,
            ) {
                Ok(()) => {
                    return true;
                }
                Err(PagingError::MemoryCacheRanOut) => {
                    for i in 0..(PAGING_CACHE_LENGTH / 2) {
                        let address = pm_manager
                            .alloc(PAGE_SIZE, true)
                            .expect("Cannot alloc memory for paging cache");
                        if !self.check_if_usable_address_range(
                            address,
                            PhysicalMemoryManager::size_to_end_address(address, PAGE_SIZE),
                        ) {
                            /* there is no function to alloc direct-mappable memory, should make...*/
                            panic!("Cannot alloc direct-map memory.")
                        }
                        if self
                            .insert_entry(
                                VirtualMemoryEntry::new(
                                    address,
                                    PhysicalMemoryManager::size_to_end_address(address, PAGE_SIZE),
                                    address,
                                    MemoryPermissionFlags::data(),
                                    MemoryOptionFlags::new(MemoryOptionFlags::WIRED),
                                ),
                                pm_manager,
                            )
                            .is_none()
                        {
                            pr_err!("Cannot associate virtual address for paging cache.");
                            return false;
                        }
                        self.reserved_memory_list.list[i] = address;
                        /* 一時的にダイレクトマップにしないといけないが.. */
                        unimplemented!();
                    }
                    self.reserved_memory_list.pointer = PAGING_CACHE_LENGTH / 2;
                    /* retry (by loop) */
                }
                Err(_) => {
                    pr_err!("Cannot associate physical address.");
                    return false;
                }
            };
        }
    }

    pub fn map_address(
        &mut self,
        physical_address: usize,
        virtual_address: Option<usize>,
        size: usize,
        permission: MemoryPermissionFlags,
        option: MemoryOptionFlags,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<usize, &str> {
        if physical_address & !PAGE_MASK != 0 {
            return Err("Physical Address is not aligned.");
        } else if size & !PAGE_MASK != 0 {
            return Err("Size is not aligned.");
        }
        let entry = if let Some(address) = virtual_address {
            if address & !PAGE_MASK != 0 {
                return Err("Virtual Address is not aligned.");
            }
            /* assume virtual address is usable. */
            /*if !self.check_if_usable_address_range(address, address + size - 1) {
                return Err("Virtual Address is not usable.");
            }*/
            VirtualMemoryEntry::new(
                address,
                PhysicalMemoryManager::size_to_end_address(address, size),
                physical_address,
                permission,
                option,
            )
        }
        /*else if self.check_if_usable_address_range(
            physical_start_address,
            physical_start_address + size - 1,
        ) {
            /* 物理・論理アドレスの合致ができるならそれが良い */
            VirtualMemoryEntry::new(
                physical_address,
                PhysicalMemoryManager::size_to_end_address(physical_start_address,size),
                physical_address,
                permission,
                option,
            )
        } */
        else if let Some(address) = self.get_free_address(size) {
            VirtualMemoryEntry::new(
                address,
                PhysicalMemoryManager::size_to_end_address(address, size),
                physical_address,
                permission,
                option,
            )
        } else {
            return Err("Virtual Address is not available.");
        };
        let address = entry.get_vm_start_address();
        let entry = self.insert_entry(entry, &mut PhysicalMemoryManager::new());
        if entry.is_none() {
            return Err("Cannot add Virtual Memory Entry.");
        }
        let entry = entry.unwrap();
        for i in 0..size / PAGE_SIZE {
            if !self.associate_address(
                physical_address + i * PAGE_SIZE,
                address + i * PAGE_SIZE,
                permission,
                pm_manager,
            ) {
                entry.set_vm_end_address(PhysicalMemoryManager::size_to_end_address(
                    entry.get_vm_start_address(),
                    (i - 1) * PAGE_SIZE,
                ));
                self.free_address(entry.get_vm_start_address(), pm_manager);
                return Err("Cannot associate physical address.");
            }
            self.update_paging(address + i * PAGE_SIZE);
        }
        Ok(address)
    }

    pub fn try_expand_size(
        &mut self,
        virtual_address: usize,
        new_size: usize,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> bool {
        if virtual_address & !PAGE_MASK != 0 {
            pr_info!("Virtual Address is not aligned.");
            return false;
        } else if new_size & !PAGE_MASK != 0 {
            pr_info!("Size is not aligned.");
            return false;
        }
        let root_entry = unsafe { &mut *(self.vm_map_entry as *mut VirtualMemoryEntry) };
        if let Some(entry) = root_entry.find_entry_mut(virtual_address) {
            if PhysicalMemoryManager::address_to_size(
                entry.get_vm_start_address(),
                entry.get_vm_end_address(),
            ) >= new_size
            {
                return true;
            }
            if let Some(next_entry_address) = entry.get_next_entry() {
                let next_entry_start_address =
                    unsafe { &*(next_entry_address as *const VirtualMemoryEntry) }
                        .get_vm_start_address();
                if PhysicalMemoryManager::size_to_end_address(
                    entry.get_vm_start_address(),
                    new_size,
                ) >= next_entry_start_address
                {
                    return false;
                }
            } else {
                if PhysicalMemoryManager::size_to_end_address(
                    entry.get_vm_start_address(),
                    new_size,
                ) >= MAX_VIRTUAL_ADDRESS
                {
                    return false;
                }
            }

            let old_size = PhysicalMemoryManager::address_to_size(
                entry.get_vm_start_address(),
                entry.get_vm_end_address(),
            );
            let not_associated_virtual_address = entry.get_vm_end_address() + 1;
            let not_associated_phsycial_address = entry.get_physical_address()
                + not_associated_virtual_address
                - entry.get_vm_start_address();
            entry.set_vm_end_address(PhysicalMemoryManager::size_to_end_address(
                entry.get_vm_start_address(),
                new_size,
            ));
            for i in 0..(new_size - old_size) / PAGE_SIZE {
                if !self.associate_address(
                    not_associated_phsycial_address + i * PAGE_SIZE,
                    not_associated_virtual_address + i * PAGE_SIZE,
                    entry.get_permission_flags(),
                    pm_manager,
                ) {
                    entry.set_vm_end_address(PhysicalMemoryManager::size_to_end_address(
                        entry.get_vm_start_address(),
                        not_associated_virtual_address + (i - 1) * PAGE_SIZE,
                    ));
                    self.free_address(entry.get_vm_start_address(), pm_manager);
                    return false;
                }
            }
            return true;
        }
        return false;
    }

    pub fn resize_memory_mapping(
        &mut self,
        virtual_address: usize,
        new_size: usize,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<usize, &str> {
        if virtual_address & !PAGE_MASK != 0 {
            return Err("Physical Address is not aligned.");
        } else if new_size & !PAGE_MASK != 0 {
            return Err("Size is not aligned.");
        }
        let root_entry = unsafe { &mut *(self.vm_map_entry as *mut VirtualMemoryEntry) };
        if let Some(entry) = root_entry.find_entry_mut(virtual_address) {
            let permission = entry.get_permission_flags();
            let physical_address = entry.get_physical_address();
            let option = entry.get_memory_option_flags();
            if !self.free_address(virtual_address, pm_manager) {
                return Err("Cannot free virtual address");
            }
            return self.map_address(
                physical_address,
                None,
                new_size,
                permission,
                option,
                pm_manager,
            );
        }
        return Err("invalid virtual address");
    }

    pub fn update_memory_permission(
        &mut self,
        vm_start_address: usize,
        new_permission: MemoryPermissionFlags,
    ) -> bool {
        self.virtual_address_to_physical_address_with_permission(vm_start_address, new_permission)
            != None
    }

    pub fn virtual_address_to_physical_address_with_permission(
        &mut self,
        virtual_address: usize,
        permission: MemoryPermissionFlags,
    ) -> Option<usize> {
        let root = unsafe { &mut *(self.vm_map_entry as *mut VirtualMemoryEntry) };
        if let Some(entry) = root.find_entry_contains_address_mut(virtual_address) {
            if entry.get_permission_flags() != permission {
                if !self.set_memory_permission(entry, permission) {
                    return None;
                }
            }
            Some(entry.get_physical_address() + virtual_address - entry.get_vm_start_address())
        } else {
            None
        }
    }

    pub fn virtual_address_to_physical_address(&self, virtual_address: usize) -> Option<usize> {
        let root = unsafe { &*(self.vm_map_entry as *const VirtualMemoryEntry) };
        if let Some(entry) = root.find_entry_contains_address(virtual_address) {
            Some(entry.get_physical_address() + virtual_address - entry.get_vm_start_address())
        } else {
            None
        }
    }

    pub fn physical_address_to_virtual_address_with_permission(
        &mut self,
        physical_address: usize,
        permission: MemoryPermissionFlags,
    ) -> Option<usize> {
        /* temporary, should be replaced by rmap */

        let root = unsafe { &mut *(self.vm_map_entry as *mut VirtualMemoryEntry) };
        if let Some(entry) = root.find_entry_contains_physical_address_mut(physical_address) {
            if entry.get_permission_flags() != permission {
                if !self.set_memory_permission(entry, permission) {
                    return None;
                }
            }
            Some(entry.get_vm_start_address() + physical_address - entry.get_physical_address())
        } else {
            None
        }
    }

    fn set_memory_permission(
        &mut self,
        entry: &mut VirtualMemoryEntry,
        permission: MemoryPermissionFlags,
    ) -> bool {
        entry.set_permission_flags(permission);

        let vm_start_address = entry.get_vm_start_address();
        for i in 0..(PhysicalMemoryManager::address_to_size(
            vm_start_address,
            entry.get_vm_end_address(),
        ) / PAGE_SIZE)
        /* should do page_round_up ? */
        {
            if self
                .page_manager
                .change_memory_permission(
                    &mut self.reserved_memory_list,
                    vm_start_address + i * PAGE_SIZE,
                    permission,
                )
                .is_err()
            {
                //do something...
                return false;
            }
        }
        return true;
    }

    pub fn get_free_address(&mut self, size: usize) -> Option<usize> {
        //think: change this function to private and make "reserve_address" function.
        let entry = unsafe { &*(self.vm_map_entry as *const VirtualMemoryEntry) };
        entry.find_usable_memory_area(size)
    }

    pub fn check_if_usable_address_range(
        &self,
        vm_start_address: usize,
        vm_end_address: usize,
    ) -> bool {
        // THINKING: rename
        let entry = unsafe { &*(self.vm_map_entry as *const VirtualMemoryEntry) };
        entry.check_usable_address_range(vm_start_address, vm_end_address)
    }

    pub fn check_if_used_memory_range(
        &self,
        vm_start_address: usize,
        vm_end_address: usize,
    ) -> bool {
        let entry = unsafe { &*(self.vm_map_entry as *const VirtualMemoryEntry) };
        if let Some(entry) = entry.find_entry(vm_start_address) {
            if entry.get_vm_end_address() == vm_end_address {
                return true;
            }
        }
        false
    }

    pub fn dump_memory_manager(&self) {
        let mut entry = unsafe { &*(self.vm_map_entry as *const VirtualMemoryEntry) };
        loop {
            kprintln!(
                "Virtual:0x{:X} Physical:0x{:X} Size:0x{:X} W:{}, U:{}, EXE:{}",
                entry.get_vm_start_address(),
                entry.get_physical_address(),
                PhysicalMemoryManager::address_to_size(
                    entry.get_vm_start_address(),
                    entry.get_vm_end_address()
                ),
                entry.get_permission_flags().write(),
                entry.get_permission_flags().user_access(),
                entry.get_permission_flags().execute()
            );
            if let Some(address) = entry.get_next_entry() {
                unsafe { entry = &*(address as *const _) };
            } else {
                break;
            }
        }
        kprintln!("----Page Manager----");
        self.page_manager.dump_table(None); // 適当
    }
}
