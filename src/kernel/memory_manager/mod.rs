/*
    Memory Manager
    This manager is the frontend of physical memory manager and page manager.
*/


pub mod physical_memory_manager;
pub mod virtual_memory_manager;
pub mod kernel_malloc_manager;

use arch::target_arch::paging::{PageManager, PAGE_SIZE};

use kernel::sync::spin_lock::Mutex;
use self::virtual_memory_manager::VirtualMemoryManager;
use self::physical_memory_manager::PhysicalMemoryManager;


pub struct MemoryManager {
    physical_memory_manager: Mutex<PhysicalMemoryManager>,
    virtual_memory_manager: VirtualMemoryManager,
}

#[derive(Clone, Eq, PartialEq, Copy)]
pub struct MemoryPermissionFlags {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
    pub user_access: bool,
}

impl MemoryManager {
    pub fn new(physical_memory_manager: Mutex<PhysicalMemoryManager>, virtual_memory_manager: VirtualMemoryManager) -> Self {
        /*カーネル領域の予約*/
        MemoryManager {
            physical_memory_manager,
            virtual_memory_manager,
        }
    }

    pub const fn new_static() -> MemoryManager {
        MemoryManager {
            physical_memory_manager: Mutex::new(PhysicalMemoryManager::new()),
            virtual_memory_manager: VirtualMemoryManager::new(),
        }
    }

    pub fn alloc_pages(&mut self, order: usize, vm_start_address: Option<usize>, permission: MemoryPermissionFlags) -> Option<usize> {
        /*TODO: lazy allocation*/
        // return physically continuous 2 ^ order pages memory.
        // this function is called by kmalloc.
        if order == 0 {
            return None;
        }
        let size = PAGE_SIZE * (1 << (order - 1));
        if let Some(vm_address) = vm_start_address {
            if !self.virtual_memory_manager.check_usable_address_range(vm_address, vm_addresss + size - 1) {
                return None;
            }
        }
        let mut physical_memory_manager = self.physical_memory_manager.lock().unwrap();
        if let Some(physical_address) = physical_memory_manager.alloc(size, true) {
            let address = vm_start_address.unwrap_or(physical_address);
            if self.virtual_memory_manager.associate_address(&mut physical_memory_manager, physical_address, address, permission) {
                PageManager::reset_paging_local(address);
                Some(address)
            } else {
                physical_memory_manager.free(physical_address, PAGE_SIZE);
                None
            }
        } else {
            None
        }
    }

    pub fn alloc_nonlinear_pages(&mut self, order: usize, vm_start_address: Option<usize>, permission: MemoryPermissionFlags) -> Option<usize> {
        /*THINK: rename*/
        // return virtually 2 ^ order pages memory.
        // this function is called by vmalloc.
        if order <= 1 {
            return self.alloc_pages(order, vm_start_address, permission);
        }
        let count = (1 << (order - 1));
        let size = PAGE_SIZE * count;
        let address = if let Some(addr) = vm_start_address {
            if !self.virtual_memory_manager.check_usable_address_range(addr, addr + size - 1) {
                return None;
            }
            addr
        } else {
            if let Some(addr) = self.virtual_memory_manager.get_free_address(size) {
                addr
            } else {
                return None;
            }
        };
        let mut pm_manager = self.physical_memory_manager.lock().unwrap();
        for i in 0..count {
            if let Some(physical_address) = pm_manager.alloc(PAGE_SIZE, true) {
                self.virtual_memory_manager.alloc_address(PAGE_SIZE, physical_address, Some(address + i * PAGE_SIZE), permission, pm_manager);
            } else {
                for j in 0..i {
                    self.virtual_memory_manager.free_address(address + i * PAGE_SIZE, pm_manager);
                }
                return None;
            }
        }
        Some(address)
    }

    pub fn free_pages(&mut self, vm_address: usize, order: usize) -> bool {
        if order == 0 {
            return false;
        }
        let count = (1 << (order - 1));
        let pm_manager = self.physical_memory_manager.lock().unwrap();
        for i in 0..count {
            if !self.virtual_memory_manager.free_address(vm_address + i * PAGE_SIZE, pm_manager) {
                return false;
            }
        }
        true
    }

    pub fn dump_memory_manager(&self) {
        if let Ok(physical_memory_manager) = self.physical_memory_manager.try_lock() {
            physical_memory_manager.dump_memory_entry();
        } else {
            println!("Can not lock Physical Memory Manager.");
        }
    }
}


impl MemoryPermissionFlags {
    pub const fn rodata() -> Self {
        Self {
            read: true,
            write: false,
            execute: false,
            user_access: false,
        }
    }
    pub const fn data() -> Self {
        Self {
            read: true,
            write: true,
            execute: false,
            user_access: false,
        }
    }
}