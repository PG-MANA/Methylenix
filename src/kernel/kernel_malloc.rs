/*
    Kernel Memory Alloc Manager
    ここでは仮想メモリ上でのメモリ確保しか関与しない
*/

use arch::target_arch::paging::{PAGE_SIZE, PAGE_MASK};

use kernel::memory_manager::physical_memory_manager::PhysicalMemoryManager;
use kernel::memory_manager::MemoryManager;

use core::mem;
use core::mem::MaybeUninit;


pub struct KernelMemoryAllocManager {
    alloc_manager: PhysicalMemoryManager,
    /*THINKING: MemoryManager*/
    used_memory_list: MaybeUninit<&'static mut [(usize, usize); PAGE_SIZE / mem::size_of::<(usize, usize)>()]>,//Temporary
}


impl KernelMemoryAllocManager {
    pub const fn new() -> Self {
        KernelMemoryAllocManager {
            alloc_manager: PhysicalMemoryManager::new(),
            used_memory_list: MaybeUninit::uninit(),
        }
    }

    pub fn init(&mut self, memory_manager: &mut MemoryManager) -> bool {
        if let Some(pool_address) = memory_manager.alloc_page(None, false, true, false) {
            self.alloc_manager.set_memory_entry_pool(pool_address, PAGE_SIZE);
        } else {
            return false;
        }
        if let Some(address) = memory_manager.alloc_page(None, false, true, false) {
            unsafe { self.used_memory_list.write(&mut *(address as *mut [(usize, usize); PAGE_SIZE / mem::size_of::<(usize, usize)>()])); }
        } else {
            return false;
        }
        for e in unsafe { self.used_memory_list.get_mut().iter_mut() } {
            *e = (0, 0);
        }
        /*Do Something...*/
        true
    }

    pub fn kmalloc(&mut self, memory_manager: &mut MemoryManager, size: usize) -> Option<usize> {
        if size == 0 {
            return None;
        }
        /*TODO: if size > PAGE_SIZE {alloc from page table}*/
        if let Some(address) = self.alloc_manager.alloc(size, false) {
            return Some(address);
        }
        /*Allocate from Memory Manager*/
        loop {
            if let Some(allocated_addres) = memory_manager.alloc_page(None/*TODO: consecutive address*/, false, false, false) {
                self.alloc_manager.define_free_memory(allocated_addres, PAGE_SIZE);
                if let Some(address) = self.alloc_manager.alloc(size, false) {
                    for e in unsafe { self.used_memory_list.get_mut().iter_mut() } {
                        if *e == (0, 0) {
                            *e = (address, size);
                            return Some(address);
                        }
                    }
                    self.alloc_manager.free(address, size);
                    break;
                }
            } else {
                break;
            }
        }
        /*TODO: Free unused memory.*/
        None
    }

    pub fn kfree(&mut self, memory_manager: &mut MemoryManager, address: usize) {
        for e in unsafe { self.used_memory_list.get_mut().iter_mut() } {
            if e.0 == address {
                if e.1 == 0 {
                    return;
                }
                self.alloc_manager.free(address, e.1);
                *e = (0, 0);
                /*TODO: return unused memory to virtual memory.*/
                return;
            }
        }
    }

    pub fn vmalloc(&mut self, memory_manager: &mut MemoryManager, size: usize) -> Option<usize> {
        if size == 0{
            return None;
        }
        let size = ((size - 1) & PAGE_MASK) + PAGE_SIZE;
        unimplemented!();//TODO
    }
}