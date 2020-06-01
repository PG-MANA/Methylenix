/*
 * Kernel Memory Allocation Manager
 * This manager is the frontend of memory allocation for structs and small size areas.
 */

use arch::target_arch::paging::PAGE_SIZE;

use kernel::memory_manager::physical_memory_manager::PhysicalMemoryManager;
use kernel::memory_manager::{MemoryManager, MemoryPermissionFlags};

use core::mem;
use core::mem::MaybeUninit;

pub struct KernelMemoryAllocManager {
    alloc_manager: PhysicalMemoryManager,
    /*THINKING: MemoryManager*/
    used_memory_list:
        MaybeUninit<&'static mut [(usize, usize); PAGE_SIZE / mem::size_of::<(usize, usize)>()]>, //Temporary
}

impl KernelMemoryAllocManager {
    pub const fn new() -> Self {
        KernelMemoryAllocManager {
            alloc_manager: PhysicalMemoryManager::new(),
            used_memory_list: MaybeUninit::uninit(),
        }
    }

    pub fn init(&mut self, m_manager: &mut MemoryManager) -> bool {
        match m_manager.alloc_pages(1, MemoryPermissionFlags::data()) {
            Ok(pool_address) => {
                self.alloc_manager
                    .set_memory_entry_pool(pool_address, PAGE_SIZE);
            }
            Err(e) => {
                pr_err!("{:?}", e);
                return false;
            }
        };
        match m_manager.alloc_pages(1, MemoryPermissionFlags::data()) {
            Ok(address) => unsafe {
                self.used_memory_list.write(
                    &mut *(address
                        as *mut [(usize, usize); PAGE_SIZE / mem::size_of::<(usize, usize)>()]),
                );
            },
            Err(e) => {
                pr_err!("{:?}", e);
                return false;
            }
        };
        for e in unsafe { self.used_memory_list.get_mut().iter_mut() } {
            *e = (0, 0);
        }
        /*Do Something...*/
        true
    }

    pub fn kmalloc(
        &mut self,
        size: usize,
        align: bool,
        m_manager: &mut MemoryManager,
    ) -> Option<usize> {
        if size == 0 {
            return None;
        }
        if size >= PAGE_SIZE {
            //TODO: do something...
            return match m_manager.alloc_pages(
                MemoryManager::size_to_order(size),
                MemoryPermissionFlags::data(),
            ) {
                Ok(address) => {
                    let aligned_size = (1 << MemoryManager::size_to_order(size)) * PAGE_SIZE;
                    if !self.add_entry_to_used_list(address, aligned_size) {
                        if let Err(e) = m_manager.free(address) {
                            pr_err!("Free memory failed Err: {:?}", e);
                        }
                        return None;
                    }
                    Some(address)
                }
                Err(e) => {
                    pr_err!("{:?}", e);
                    None
                }
            };
        }
        if let Some(address) = self.alloc_manager.alloc(size, align) {
            if !self.add_entry_to_used_list(address, size) {
                self.alloc_manager.free(address, size, false);
                return None;
            }
            return Some(address);
        }

        /* alloc from Memory Manager */
        if let Ok(allocated_address) = m_manager.alloc_pages(0, MemoryPermissionFlags::data()) {
            self.alloc_manager.free(allocated_address, PAGE_SIZE, true);
            return self.kmalloc(size, align, m_manager);
        }
        /*TODO: Free unused memory.*/
        None
    }

    pub fn vmalloc(
        &mut self,
        size: usize,
        align: bool,
        m_manager: &mut MemoryManager,
    ) -> Option<usize> {
        if size == 0 {
            return None;
        }
        if size < PAGE_SIZE {
            return self.kmalloc(size, align, m_manager);
        }

        match m_manager.alloc_nonlinear_pages(
            MemoryManager::size_to_order(size),
            MemoryPermissionFlags::data(),
        ) {
            Ok(address) => {
                if self.add_entry_to_used_list(address, size) {
                    Some(address)
                } else {
                    if let Err(e) = m_manager.free(address) {
                        pr_err!("Free memory failed Err: {:?}", e);
                    }
                    None
                }
            }
            Err(e) => {
                pr_err!("{:?}", e);
                None
            }
        }
    }

    pub fn kfree(&mut self, address: usize, _m_manager: &mut MemoryManager) {
        for e in unsafe { self.used_memory_list.get_mut().iter_mut() } {
            if e.0 == address {
                if e.1 == 0 {
                    return;
                }
                self.alloc_manager.free(address, e.1, false);
                *e = (0, 0);
                /*TODO: return unused memory to virtual memory.*/
                return;
            }
        }
    }

    pub fn vfree(&mut self, address: usize, m_manager: &mut MemoryManager) {
        for e in unsafe { self.used_memory_list.get_mut().iter_mut() } {
            if e.0 == address {
                if e.1 == 0 {
                    return;
                }
                if e.1 < PAGE_SIZE {
                    return self.kfree(address, m_manager);
                }
                if let Err(err) = m_manager.free(e.0) {
                    pr_err!("Free memory failed Err: {:?}", err);
                }
                self.alloc_manager.free(e.0, e.1, false);
                *e = (0, 0);
                return;
            }
        }
    }

    fn add_entry_to_used_list(&mut self, address: usize, size: usize) -> bool {
        for e in unsafe { self.used_memory_list.get_mut().iter_mut() } {
            if *e == (0, 0) {
                *e = (address, size);
                return true;
            }
        }
        false
    }
}
