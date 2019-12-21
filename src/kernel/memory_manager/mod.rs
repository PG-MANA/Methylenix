/*
    Memory Manager
*/


pub mod physical_memory_manager;


use arch::target_arch::paging::{PageManager, PAGE_SIZE};

use kernel::spin_lock::Mutex;
use kernel::memory_manager::physical_memory_manager::PhysicalMemoryManager;

use core::borrow::BorrowMut;


pub struct MemoryManager {
    physical_memory_manager: Mutex<PhysicalMemoryManager>,
    page_manager: PageManager,
}

impl MemoryManager {
    pub fn new(physical_memory_manager: Mutex<PhysicalMemoryManager>, page_manager: PageManager) -> MemoryManager {
        /*カーネル領域の予約*/
        MemoryManager {
            physical_memory_manager,
            page_manager,
        }
    }

    pub const fn new_static() -> MemoryManager {
        MemoryManager {
            physical_memory_manager: Mutex::new(PhysicalMemoryManager::new()),
            page_manager: PageManager::new_static(),
        }
    }

    pub fn alloc_physical_page(&mut self, should_executable: bool, should_writable: bool, should_user_accessible: bool) -> Option<usize> {
        self.alloc_page(None, should_executable, should_writable, should_user_accessible)
    }

    pub fn alloc_page(&mut self, linear_address: Option<usize>, should_executable: bool, should_writable: bool, should_user_accessible: bool) -> Option<usize> {
        /*TODO: lazy allocation*/
        let mut physical_memory_manager = self.physical_memory_manager.lock().unwrap();
        if let Some(physical_address) = physical_memory_manager.alloc(PAGE_SIZE, true) {
            let address = linear_address.unwrap_or(physical_address);
            if self.page_manager.associate_address(&mut physical_memory_manager, physical_address, address, should_executable, should_writable, should_user_accessible) {
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

    pub fn associate_address(&mut self, physical_address: usize, linear_address: usize, is_code: bool, is_writable: bool, is_user_accessible: bool) -> bool {
        self.page_manager.associate_address(self.physical_memory_manager.lock().unwrap().borrow_mut(), physical_address, linear_address, is_code, is_writable, is_user_accessible)
    }

    pub fn free(&mut self, linear_address: usize, size: usize) {
        /*Temporally*/
        /*TODO: make linear address process*/
        self.physical_memory_manager.lock().unwrap().free(linear_address, size);
    }

    pub fn dump_memory_manager(&self) {
        if let Ok(physical_memory_manager) = self.physical_memory_manager.try_lock() {
            physical_memory_manager.dump_memory_entry();
        } else {
            println!("Can not lock Physical Memory Manager.");
        }
    }
}
