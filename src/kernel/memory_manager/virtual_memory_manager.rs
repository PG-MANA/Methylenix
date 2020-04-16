/*
 * Virtual Memory Manager
 * This manager maintains memory map and controls page_manager.
 * The address and size are rounded up to an integral number of PAGE_SIZE.
*/

use arch::target_arch::paging::PageManager;
use arch::target_arch::paging::{PAGE_MASK, PAGE_SIZE, PAGING_CACHE_LENGTH};

use kernel::memory_manager::physical_memory_manager::PhysicalMemoryManager;
use kernel::memory_manager::virtual_memory_entry::VirtualMemoryEntry;
use kernel::memory_manager::{FreePageList, MemoryPermissionFlags};

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
        //後で解放する気があるならPAGE_SIZEごとに分けるのが良さそう?
        let addr = pm_manager
            .alloc(PAGE_SIZE * PAGING_CACHE_LENGTH, true)
            .expect("Cannot alloc memory for Virtual Memory Cache");
        let vme_for_cache = VirtualMemoryEntry::new(
            addr,
            addr + PAGE_SIZE * PAGING_CACHE_LENGTH - 1,
            addr,
            MemoryPermissionFlags::data(),
        );
        for i in 0..PAGING_CACHE_LENGTH {
            self.reserved_memory_list.list[i] = addr + PAGE_SIZE * i;
        }
        self.reserved_memory_list.pointer = PAGING_CACHE_LENGTH;
        /* ページング反映とエントリー追加は後でやる*/

        /* set up page_manager */
        if !self.page_manager.init(&mut self.reserved_memory_list) {
            return false;
        }

        /* set up memory pool */
        self.entry_pool = if let Some(address) = pm_manager.alloc(PAGE_SIZE * 4, true) {
            address
        } else {
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
        self.entry_pool_size = PAGE_SIZE * 4;
        for i in 0..(self.entry_pool_size / PAGE_SIZE) {
            self.page_manager.associate_address(
                &mut self.reserved_memory_list,
                self.entry_pool + i * PAGE_SIZE,
                self.entry_pool + i * PAGE_SIZE,
                MemoryPermissionFlags::data(),
            );
        }
        unsafe {
            *(self.entry_pool as *mut VirtualMemoryEntry) = VirtualMemoryEntry::new(
                self.entry_pool,
                self.entry_pool + self.entry_pool_size - 1,
                self.entry_pool,
                MemoryPermissionFlags::data(),
            );
        }

        /* insert cached_memory_list entry */
        self.insert_entry(vme_for_cache, pm_manager);
        self.vm_map_entry =
            unsafe { &mut *(self.vm_map_entry as *mut VirtualMemoryEntry) }.adjust_entries();
        for i in 0..self.reserved_memory_list.pointer {
            let physical_address = self.reserved_memory_list.list[i];
            self.page_manager.associate_address(
                &mut self.reserved_memory_list,
                physical_address,
                physical_address,
                MemoryPermissionFlags::data(),
            );
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
    ) -> bool {
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
                    return false;
                }
                self.vm_map_entry =
                    unsafe { &mut (*(self.vm_map_entry as *mut VirtualMemoryEntry)) }
                        .adjust_entries();
                return true;
            }
        }
        //TODO: realloc entry
        return false;
    }

    pub fn alloc_address(
        &mut self,
        size: usize,
        physical_start_address: usize,
        vm_start_address: Option<usize>,
        permission: MemoryPermissionFlags,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Option<usize> {
        //NOTE: ページキャッシュの更新は行わない
        if physical_start_address & !PAGE_MASK != 0 {
            pr_err!("Physical Address is not aligned.");
            return None;
        } else if size & !PAGE_MASK != 0 {
            pr_err!("Size is not aligned.");
            return None;
        }
        let entry = if let Some(address) = vm_start_address {
            if address & !PAGE_MASK != 0 {
                pr_err!("Virtual Address is not aligned.");
                return None;
            }
            if !self.check_if_usable_address_range(address, address + size - 1) {
                pr_warn!("Virtual Address is not usable.");
                return None;
            }
            VirtualMemoryEntry::new(
                address,
                address + size - 1,
                physical_start_address,
                permission,
            )
        } else if self.check_if_usable_address_range(
            physical_start_address,
            physical_start_address + size - 1,
        ) {
            /* 物理・論理アドレスの合致ができるならそれが良い */
            VirtualMemoryEntry::new(
                physical_start_address,
                physical_start_address + size - 1,
                physical_start_address,
                permission,
            )
        } else if let Some(address) = self.get_free_address(size) {
            VirtualMemoryEntry::new(
                address,
                address + size - 1,
                physical_start_address,
                permission,
            )
        } else {
            pr_warn!("Virtual Address is not available.");
            return None;
        };
        let address = entry.get_vm_start_address();
        if !self.insert_entry(entry, pm_manager) {
            pr_warn!("Cannot add Virtual Memory Entry.");
            return None;
        }
        for i in 0..size / PAGE_SIZE {
            if !self.page_manager.associate_address(
                &mut self.reserved_memory_list,
                physical_start_address + i * PAGE_SIZE,
                address + i * PAGE_SIZE,
                permission,
            ) {
                panic!("Cannot associate physical address."); // 後で巻き戻してdelete_entryしてNoneする処理を追加
            }
        }
        Some(address)
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
        let root_entry = unsafe { &mut *(self.vm_map_entry as *mut VirtualMemoryEntry) };
        let root_physical_address = root_entry.get_physical_address();
        if let Some(entry) = root_entry.find_entry_mut(vm_start_address) {
            if entry.get_physical_address() == root_physical_address {
                panic!("VirtualMemoryManager: Cannot delete root entry.");
            }
            let vm_start_address = entry.get_vm_start_address();
            let size = entry.get_vm_end_address() - vm_start_address + 1;
            let physical_address = entry.get_physical_address();
            entry.delete_entry();
            root_entry.adjust_entries();
            for i in 0..(size / PAGE_SIZE) {
                self.page_manager.unassociate_address(
                    vm_start_address + i * PAGE_SIZE,
                    &mut self.reserved_memory_list,
                );
            }
            pm_manager.free(physical_address, size, false); //should do this at MemoryManager...?
            true
        } else {
            false
        }
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
            if !self.page_manager.change_memory_permission(
                &mut self.reserved_memory_list,
                vm_start_address + i * PAGE_SIZE,
                permission,
            ) {
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
                entry.get_permission_flags().write,
                entry.get_permission_flags().user_access,
                entry.get_permission_flags().execute
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

    pub const fn size_to_order(size: usize) -> usize {
        if size == 0 {
            return 0;
        }
        let mut page_count = (((size - 1) & PAGE_MASK) / PAGE_SIZE) + 1;
        let mut order = if page_count & (page_count - 1) == 0 {
            0usize
        } else {
            1usize
        };
        while page_count != 0 {
            page_count >>= 1;
            order += 1;
        }
        order
    }
}
