/*
 * Memory Manager
 * This manager is the frontend of physical memory manager and page manager.
 */

pub mod kernel_malloc_manager;
pub mod physical_memory_manager;
pub mod virtual_memory_entry;
pub mod virtual_memory_manager;

use arch::target_arch::paging::{PAGE_MASK, PAGE_SIZE, PAGING_CACHE_LENGTH};

use self::physical_memory_manager::PhysicalMemoryManager;
use self::virtual_memory_manager::VirtualMemoryManager;
use kernel::sync::spin_lock::Mutex;

pub struct MemoryManager {
    physical_memory_manager: Mutex<PhysicalMemoryManager>,
    virtual_memory_manager: VirtualMemoryManager,
}

#[derive(Clone, Eq, PartialEq, Copy)]
pub struct MemoryPermissionFlags {
    flags: u8,
}

pub enum MemoryOption {}

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
                self.virtual_memory_manager
                    .update_paging(address + i * PAGE_SIZE);
            } else {
                for j in 0..i {
                    self.virtual_memory_manager
                        .free_address(address + j * PAGE_SIZE, &mut pm_manager);
                    self.virtual_memory_manager
                        .update_paging(address + j * PAGE_SIZE);
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

    pub fn reserve_memory(
        &mut self,
        physical_address: usize,
        virtual_address: usize,
        size: usize,
        permission: MemoryPermissionFlags,
        physical_address_may_be_reserved: bool,
        virtual_memory_may_be_reserved: bool,
    ) -> bool {
        if physical_address & !PAGE_MASK != 0 {
            pr_err!("Physical Address is not aligned.");
            return false;
        } else if virtual_address & !PAGE_MASK != 0 {
            pr_err!("Virtual Address is not aligned.");
            return false;
        } else if size & !PAGE_MASK != 0 {
            pr_err!("Size is not aligned.");
            return false;
        }
        if let Ok(mut pm_manager) = self.physical_memory_manager.try_lock() {
            let mut allocated_memory = false;
            if pm_manager.reserve_memory(physical_address, size, false) {
                allocated_memory = true;
            } else {
                if !physical_address_may_be_reserved {
                    pr_err!("Cannot allocate physical address.");
                    return false;
                }
            }
            if self.virtual_memory_manager.alloc_address(
                size,
                physical_address,
                Some(virtual_address),
                permission,
                &mut pm_manager,
            ) != Some(virtual_address)
            {
                if virtual_memory_may_be_reserved {
                    if self
                        .virtual_memory_manager
                        .virtual_address_to_physical_address(virtual_address)
                        == Some(physical_address)
                    {
                        if self
                            .virtual_memory_manager
                            .update_memory_permission(virtual_address, permission)
                        {
                            self.virtual_memory_manager.update_paging(virtual_address);
                            return true;
                        }
                    }
                }
                if allocated_memory {
                    pm_manager.free(physical_address, size, false);
                }
                pr_err!("Cannot reserve memory.");
                return false;
            }
            self.virtual_memory_manager.update_paging(virtual_address);
            return true;
        } else {
            return false;
        }
    }

    pub fn get_vm_address(
        &mut self,
        physical_address: usize,
        required_permission: MemoryPermissionFlags,
        shoud_reserve_if_not_avalable: bool,
        physical_address_may_be_reserved: bool,
    ) -> Option<usize> {
        /* use reverse map to search virtual address by O(1) */
        let aligned_physical_address = physical_address & PAGE_MASK;
        if let Some(virtual_address) = self
            .virtual_memory_manager
            .physical_address_to_virtual_address_with_permission(
                physical_address,
                required_permission,
            )
        {
            return Some(virtual_address);
        } else if shoud_reserve_if_not_avalable {
            if let Ok(mut pm_manager) = self.physical_memory_manager.try_lock() {
                if pm_manager.reserve_memory(aligned_physical_address, PAGE_SIZE, false)
                    && !physical_address_may_be_reserved
                {
                    return None;
                }
                if let Some(vm_address) = self.virtual_memory_manager.alloc_address(
                    PAGE_SIZE,
                    aligned_physical_address,
                    None,
                    required_permission,
                    &mut pm_manager,
                ) {
                    return Some(vm_address + physical_address - aligned_physical_address);
                }
            }
        }
        return None;
    }

    pub fn set_paging_table(&mut self) {
        self.virtual_memory_manager.flush_paging();
    }

    pub fn dump_memory_manager(&self) {
        if let Ok(physical_memory_manager) = self.physical_memory_manager.try_lock() {
            kprintln!("----Physical Memory Entries Dump----");
            physical_memory_manager.dump_memory_entry();
            kprintln!("----Physical Memory Entries Dump End----");
        } else {
            kprintln!("Can not lock Physical Memory Manager.");
        }
        kprintln!("----Virtual Memory Entries Dump----");
        self.virtual_memory_manager.dump_memory_manager();
        kprintln!("----Virtual Memory Entries Dump End----");
    }

    pub const fn page_round_up(address: usize, size: usize) -> (usize /*address*/, usize /*size*/) {
        if size == 0 && (address & PAGE_MASK) == 0 {
            (address, 0)
        } else {
            (
                (address & PAGE_MASK),
                (((size + (address - (address & PAGE_MASK)) - 1) & PAGE_MASK) + PAGE_SIZE),
            )
        }
    }
}

impl MemoryPermissionFlags {
    pub const fn new(read: bool, write: bool, execute: bool, user_access: bool) -> Self {
        Self {
            flags: ((read as u8) << 0)
                | ((write as u8) << 1)
                | ((execute as u8) << 2)
                | ((user_access as u8) << 3),
        }
    }

    pub const fn rodata() -> Self {
        Self::new(true, false, false, false)
    }

    pub const fn data() -> Self {
        Self::new(true, true, false, false)
    }

    pub fn read(&self) -> bool {
        self.flags & (1 << 0) != 0
    }

    pub fn write(&self) -> bool {
        self.flags & (1 << 1) != 0
    }

    pub fn execute(&self) -> bool {
        self.flags & (1 << 2) != 0
    }

    pub fn user_access(&self) -> bool {
        self.flags & (1 << 3) != 0
    }
}
