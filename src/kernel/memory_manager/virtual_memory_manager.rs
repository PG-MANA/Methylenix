/*
 * Virtual Memory Manager
 * This manager maintains memory map and controls page_manager.
 * The address and size are rounded up to an integral number of PAGE_SIZE.
*/


use arch::target_arch::paging::PageManager;
use arch::target_arch::paging::{PAGE_MASK, PAGE_SIZE};

use kernel::memory_manager::physical_memory_manager::PhysicalMemoryManager;
use kernel::memory_manager::MemoryPermissionFlags;

use core::mem;

/*usize entries are temporary members.*/
pub struct VirtualMemoryManager {
    vm_map_entry: usize,
    is_system_vm: bool,
    page_manager: PageManager,
    entry_pool: usize,
    entry_pool_size: usize,
}

#[derive(Clone, Copy)]
pub struct VirtualMemoryEntry {
    next_entry: Option<usize>,
    prev_entry: Option<usize>,
    start_address: usize,
    physical_start_address: usize,
    /*temporary*/
    end_address: usize,
    is_shared: bool,
    should_cow: bool,
    permission_flags: MemoryPermissionFlags,
}
// ADD: thread chain

impl VirtualMemoryManager {
    pub const MAX_VIRTUAL_ADDRESS: usize = usize::MAX;

    pub const fn new() -> Self {
        Self {
            vm_map_entry: 0,
            is_system_vm: false,
            page_manager: PageManager::new_static(),
            entry_pool: 0,
            entry_pool_size: 0,
        }
    }

    pub fn init(&mut self, is_system_vm: bool, page_manager: PageManager, pm_manager: &mut PhysicalMemoryManager) -> bool {
        self.page_manager = page_manager;
        self.is_system_vm = is_system_vm;
        let pool = pm_manager.alloc(PAGE_SIZE, true);
        if pool.is_none() {
            return false;
        }
        self.entry_pool = pool.unwrap();
        self.entry_pool_size = PAGE_SIZE;
        self.page_manager.associate_address(pm_manager, self.entry_pool, self.entry_pool, MemoryPermissionFlags::data());
        for i in 0..(self.entry_pool_size / VirtualMemoryEntry::ENTRY_SIZE) {
            unsafe { (*((self.entry_pool + i * VirtualMemoryEntry::ENTRY_SIZE) as *mut VirtualMemoryEntry)).set_disabled() }
        }

        unsafe {
            *(self.entry_pool as *mut VirtualMemoryEntry) = VirtualMemoryEntry {
                next_entry: Some(0),
                prev_entry: Some(0),
                start_address: self.entry_pool,
                end_address: self.entry_pool + self.entry_pool_size - 1,
                physical_start_address: self.entry_pool,
                is_shared: false,
                should_cow: false,
                permission_flags: MemoryPermissionFlags::data(),
            };
        }
        true
    }

    pub fn alloc_address(&mut self, size: usize, physical_start_address: usize, vm_start_address: Option<usize>, permission: MemoryPermissionFlags, pm_manager: &mut PhysicalMemoryManager) -> Option<usize> {
        if size & !PAGE_MASK != 0 {
            return None;
        }
        let entry = if let Some(address) = vm_start_address {
            if !self.check_usable_address_range(address, address + size - 1) {
                return None;
            }
            VirtualMemoryEntry::new(address, address + size - 1, physical_start_address, permission)
        } else if let Some(address) = self.get_free_address(size) {
            VirtualMemoryEntry::new(address, address + size - 1, physical_start_address, permission)
        } else {
            return None;
        };
        let address = entry.start_address;
        self.insert_entry(entry, pm_manager);
        Some(address)
    }

    pub fn free_address(&mut self, vm_start_address: usize, pm_manager: &mut PhysicalMemoryManager) -> bool {
        //TODO: 結合されていたエントリの分離処理
        if let Some(entry) = self.find_entry_mut(vm_start_address) {
            let physical_address = entry.physical_start_address;
            let size = entry.end_address - entry.start_address + 1;
            self.delete_entry(entry, pm_manager);
            for i in 0..(size / PAGE_SIZE) {
                self.page_manager.unassociate_address(vm_start_address + i * PAGE_SIZE, pm_manager);
            }
            pm_manager.free(physical_address, size);
            true
        } else {
            false
        }
    }

    pub fn get_free_address(&mut self, size: usize) -> Option<usize> {
        //think: change this function to private and make "reserve_address" function.
        let mut prev_entry = unsafe { &*(self.vm_map_entry as *const VirtualMemoryEntry) };
        while let Some(next) = prev_entry.next_entry {
            let next_entry = unsafe { &mut *(next as *mut VirtualMemoryEntry) };
            if prev_entry.end_address + 1 + size < next_entry.start_address {
                return Some(prev_entry.end_address + 1);
            }
            prev_entry = next_entry;
        }
        if prev_entry.end_address + 1 + size >= Self::MAX_VIRTUAL_ADDRESS {
            return None;
        }
        Some(prev_entry.end_address + 1)
    }

    fn insert_entry(&mut self, vm_entry: VirtualMemoryEntry, pm_manager: &mut PhysicalMemoryManager) -> bool {
        if (vm_entry.start_address & !PAGE_MASK != 0) || ((vm_entry.end_address + 1) & !PAGE_MASK != 0) {
            return false;
        }
        for i in 0..(self.entry_pool_size / VirtualMemoryEntry::ENTRY_SIZE) {
            let e = (self.entry_pool + i * VirtualMemoryEntry::ENTRY_SIZE) as *mut VirtualMemoryEntry;
            if unsafe { &*e }.is_disabled() {
                unsafe { *e = vm_entry; }
                self.adjust_entries(pm_manager);
                self.page_manager.associate_address(pm_manager, vm_entry.physical_start_address, vm_entry.start_address, vm_entry.permission_flags);
                self.page_manager.reset_paging();// THINK: reset_paging_local
                return true;
            }
        }
        false
    }

    fn delete_entry(&mut self, target_entry: &mut VirtualMemoryEntry, pm_manager: &mut PhysicalMemoryManager) {
        target_entry.unchain();
        target_entry.set_disabled();
        self.adjust_entries(pm_manager);
    }

    pub fn check_usable_address_range(&self, vm_start_address: usize, vm_end_address: usize) -> bool {
        // THINKING: rename
        let mut entry = unsafe { &*(self.vm_map_entry as *const VirtualMemoryEntry) };
        while entry.end_address >= vm_end_address {
            if (entry.start_address <= vm_start_address && entry.end_address >= vm_start_address) ||
                (entry.start_address <= vm_start_address && entry.end_address >= vm_start_address) {
                return false;
            }
            if let Some(e) = entry.next_entry {
                entry = unsafe { &*(e as *const _) };
            } else {
                break;
            }
        }
        true
    }

    fn find_entry(&self, vm_start_address: usize) -> Option<&VirtualMemoryEntry> {
        //TODO: Tree
        let mut entry = unsafe { &*(self.vm_map_entry as *const VirtualMemoryEntry) };
        while entry.start_address <= vm_start_address {
            if entry.end_address >= vm_start_address {
                return Some(entry);
            }
            if let Some(e) = entry.next_entry {
                entry = unsafe { &*(e as *const _) };
            } else {
                break;
            }
        }
        None
    }

    fn find_entry_mut(&mut self, vm_start_address: usize) -> Option<&'static mut VirtualMemoryEntry> {
        //TODO: Tree
        let mut entry = unsafe { &mut *(self.vm_map_entry as *mut VirtualMemoryEntry) };
        while entry.start_address <= vm_start_address {
            if entry.end_address >= vm_start_address {
                return Some(entry);
            }
            if let Some(e) = entry.next_entry {
                entry = unsafe { &mut *(e as *mut _) };
            } else {
                break;
            }
        }
        None
    }

    fn adjust_entries(&mut self, pm_manager: &mut PhysicalMemoryManager) {
        //TODO: 同一属性の連続したエントリの結合
        return;
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

impl VirtualMemoryEntry {
    pub const ENTRY_SIZE: usize = mem::size_of::<Self>();

    pub const fn new(
        vm_start_address: usize,
        vm_end_address: usize,
        physical_start_address: usize,
        permission: MemoryPermissionFlags,
    ) -> Self {
        Self {
            prev_entry: None,
            next_entry: None,
            start_address: vm_start_address,
            end_address: vm_end_address,
            physical_start_address,
            is_shared: false,
            should_cow: false,
            permission_flags: permission,
        }
    }

    pub fn is_disabled(&self) -> bool {
        self.start_address == 0 && self.end_address == 0 && self.physical_start_address == 0
    }

    pub fn set_disabled(&mut self) {
        self.start_address = 0;
        self.end_address = 0;
        self.physical_start_address = 0;
    }

    pub fn chain_after_me(&mut self, entry: &mut Self) {
        self.next_entry = Some(entry as *mut Self as usize);
        unsafe { (&mut *(entry as *mut Self)).prev_entry = Some(self as *mut Self as usize); }
    }

    pub fn unchain(&mut self) {
        if let Some(prev) = self.prev_entry {
            unsafe { &mut *(prev as *mut Self) }.next_entry = self.next_entry;
        }
        if let Some(next) = self.next_entry {
            unsafe { &mut *(next as *mut Self) }.prev_entry = self.prev_entry;
        }
        self.prev_entry = None;
        self.next_entry = None;
    }
}