/*
    Memory Manager
    This manager is the frontend of physical memory manager and page manager.
*/

pub mod kernel_malloc_manager;
pub mod physical_memory_manager;
pub mod virtual_memory_entry;
pub mod virtual_memory_manager;

use arch::target_arch::paging::{PAGE_SIZE, PAGING_CACHE_LENGTH};

use self::physical_memory_manager::PhysicalMemoryManager;
use self::virtual_memory_manager::VirtualMemoryManager;
use kernel::sync::spin_lock::Mutex;

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

pub struct FreePageList {
    pub list: [usize; PAGING_CACHE_LENGTH],
    pub pointer: usize,
}

impl MemoryManager {
    pub fn new(
        physical_memory_manager: Mutex<PhysicalMemoryManager>,
        virtual_memory_manager: VirtualMemoryManager,
    ) -> Self {
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

    pub fn alloc_pages(
        &mut self,
        order: usize,
        vm_start_address: Option<usize>,
        permission: MemoryPermissionFlags,
    ) -> Option<usize> {
        /*TODO: lazy allocation*/
        // return physically continuous 2 ^ order pages memory.
        // this function is called by kmalloc.
        let size = PAGE_SIZE * (1 << order);
        if let Some(vm_address) = vm_start_address {
            if !self
                .virtual_memory_manager
                .check_if_usable_address_range(vm_address, vm_address + size - 1)
            {
                return None;
            }
        }
        let mut physical_memory_manager = self.physical_memory_manager.lock().unwrap();
        if let Some(physical_address) = physical_memory_manager.alloc(size, true) {
            if let Some(address) = self.virtual_memory_manager.alloc_address(
                size,
                physical_address,
                vm_start_address,
                permission,
                &mut physical_memory_manager,
            ) {
                self.virtual_memory_manager.update_paging(address);
                Some(address)
            } else {
                physical_memory_manager.free(physical_address, size, false);
                None
            }
        } else {
            None
        }
    }

    pub fn alloc_nonlinear_pages(
        &mut self,
        order: usize,
        vm_start_address: Option<usize>,
        permission: MemoryPermissionFlags,
    ) -> Option<usize> {
        /*THINK: rename*/
        // return virtually 2 ^ order pages memory.
        // this function is called by vmalloc.
        // vfreeの際に全てのメモリが開放されないバグを含んでいる
        if order == 0 {
            return self.alloc_pages(order, vm_start_address, permission);
        }
        let count = 1 << order;
        let size = PAGE_SIZE * count;
        let address = if let Some(addr) = vm_start_address {
            if !self
                .virtual_memory_manager
                .check_if_usable_address_range(addr, addr + size - 1)
            {
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
                self.virtual_memory_manager.alloc_address(
                    PAGE_SIZE,
                    physical_address,
                    Some(address + i * PAGE_SIZE),
                    permission,
                    &mut pm_manager,
                );
            } else {
                for j in 0..i {
                    self.virtual_memory_manager
                        .free_address(address + j * PAGE_SIZE, &mut pm_manager);
                }
                return None;
            }
        }
        Some(address)
    }

    pub fn free_pages(&mut self, vm_address: usize, _order: usize) -> bool {
        //let count = 1 << order;
        let mut pm_manager = self.physical_memory_manager.lock().unwrap();
        if !self
            .virtual_memory_manager
            .free_address(vm_address, &mut pm_manager)
        {
            return false;
        }
        //物理メモリの開放はfree_addressでやっているが本来はここでやるべきか?
        true
    }

    pub fn free_physical_memory(&mut self, physical_address: usize, size: usize) -> bool {
        /* initializing use only */
        if let Ok(mut pm_manager) = self.physical_memory_manager.try_lock() {
            pm_manager.free(physical_address, size, false)
        } else {
            false
        }
    }

    pub fn set_paging_table(&mut self) {
        self.virtual_memory_manager.flush_paging();
    }

    pub fn dump_memory_manager(&self) {
        if let Ok(physical_memory_manager) = self.physical_memory_manager.try_lock() {
            println!("----Physical Memory Entries Dump----");
            physical_memory_manager.dump_memory_entry();
            println!("----Physical Memory Entries Dump End----");
        } else {
            println!("Can not lock Physical Memory Manager.");
        }
        println!("----Virtual Memory Entries Dump----");
        self.virtual_memory_manager.dump_memory_manager();
        println!("----Virtual Memory Entries Dump End----");
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
