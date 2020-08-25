/*
 * Kernel Memory Allocation Manager
 * This manager is the frontend of memory allocation for structs and small size areas.
 */

use arch::target_arch::paging::{PAGE_MASK, PAGE_SIZE};

use kernel::memory_manager::data_type::{Address, MOrder, MSize, VAddress};
use kernel::memory_manager::physical_memory_manager::PhysicalMemoryManager;
use kernel::memory_manager::{MemoryManager, MemoryPermissionFlags};
use kernel::sync::spin_lock::Mutex;

use core::mem;
use core::mem::MaybeUninit;

pub struct KernelMemoryAllocManager {
    alloc_manager: PhysicalMemoryManager,
    /*THINKING: MemoryManager*/
    used_memory_list: MaybeUninit<
        &'static mut [(VAddress, MSize); PAGE_SIZE / mem::size_of::<(VAddress, MSize)>()],
    >, //Temporary
}

impl KernelMemoryAllocManager {
    pub const fn new() -> Self {
        KernelMemoryAllocManager {
            alloc_manager: PhysicalMemoryManager::new(),
            used_memory_list: MaybeUninit::uninit(),
        }
    }

    pub fn init(&mut self, m_manager: &mut MemoryManager) -> bool {
        match m_manager.alloc_pages(1.into(), MemoryPermissionFlags::data()) {
            Ok(pool_address) => {
                self.alloc_manager
                    .set_memory_entry_pool(pool_address.to_usize(), PAGE_SIZE);
            }
            Err(e) => {
                pr_err!("{:?}", e);
                return false;
            }
        };
        match m_manager.alloc_pages(1.into(), MemoryPermissionFlags::data()) {
            Ok(address) => unsafe {
                self.used_memory_list.write(
                    &mut *(address.to_usize()
                        as *mut [(VAddress, MSize);
                            PAGE_SIZE / mem::size_of::<(VAddress, MSize)>()]),
                );
            },
            Err(e) => {
                pr_err!("{:?}", e);
                return false;
            }
        };
        for e in unsafe { self.used_memory_list.get_mut().iter_mut() } {
            *e = (0.into(), 0.into());
        }
        /*Do Something...*/
        true
    }

    pub fn kmalloc(
        &mut self,
        size: MSize,
        align_order: MOrder,
        m_manager: &Mutex<MemoryManager>,
    ) -> Option<VAddress> {
        if size.is_zero() {
            return None;
        }
        if size >= PAGE_SIZE.into() {
            let mut locked_m_manager = m_manager.lock().unwrap();
            return match locked_m_manager
                .alloc_pages(size.to_order(None), MemoryPermissionFlags::data())
            {
                Ok(address) => {
                    let aligned_size =
                        MSize::from(((size - MSize::from(1)) & PAGE_MASK) + PAGE_SIZE);
                    if !self.add_entry_to_used_list(address, aligned_size) {
                        if let Err(e) = locked_m_manager.free(address) {
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
        if let Some(address) = self.alloc_manager.alloc(size, align_order) {
            if !self.add_entry_to_used_list(address.to_direct_mapped_v_address(), size) {
                self.alloc_manager.free(address, size, false);
                return None;
            }
            return Some(address.to_direct_mapped_v_address());
        }

        let mut locked_m_manager = m_manager.lock().unwrap();
        /* alloc from Memory Manager */
        if let Ok(allocated_address) =
            locked_m_manager.alloc_pages(0.into(), MemoryPermissionFlags::data())
        {
            self.alloc_manager.free(
                allocated_address.to_direct_mapped_p_address(),
                PAGE_SIZE.into(),
                true,
            );
            drop(locked_m_manager);
            return self.kmalloc(size, align_order, m_manager);
        }
        /*TODO: Free unused memory.*/
        None
    }

    pub fn vmalloc(
        &mut self,
        size: MSize,
        align_order: MOrder,
        m_manager: &Mutex<MemoryManager>,
    ) -> Option<VAddress> {
        if size.is_zero() {
            return None;
        }
        if size < PAGE_SIZE.into() {
            return self.kmalloc(size, align_order, m_manager);
        }

        match m_manager
            .lock()
            .unwrap()
            .alloc_nonlinear_pages(size.to_order(None), MemoryPermissionFlags::data())
        {
            Ok(address) => {
                if self.add_entry_to_used_list(address, size) {
                    Some(address)
                } else {
                    if let Err(e) = m_manager.lock().unwrap().free(address) {
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

    pub fn kfree(&mut self, address: VAddress, _m_manager: &Mutex<MemoryManager>) {
        for e in unsafe { self.used_memory_list.get_mut().iter_mut() } {
            if e.0 == address {
                if e.1.is_zero() {
                    return;
                }
                self.alloc_manager
                    .free(e.0.to_direct_mapped_p_address(), e.1, false);
                *e = (0.into(), 0.into());
                /*TODO: return unused memory to virtual memory.*/
                return;
            }
        }
    }

    pub fn vfree(&mut self, address: VAddress, m_manager: &Mutex<MemoryManager>) {
        for e in unsafe { self.used_memory_list.get_mut().iter_mut() } {
            if e.0 == address {
                if e.1.is_zero() {
                    return;
                }
                if e.1 < PAGE_SIZE.into() {
                    return self.kfree(address, m_manager);
                }
                if let Err(err) = m_manager.lock().unwrap().free(address) {
                    pr_err!("Free memory failed Err: {:?}", err);
                }
                self.alloc_manager
                    .free(e.0.to_direct_mapped_p_address(), e.1, false);
                *e = (0.into(), 0.into());
                return;
            }
        }
    }

    fn add_entry_to_used_list(&mut self, address: VAddress, size: MSize) -> bool {
        for e in unsafe { self.used_memory_list.get_mut().iter_mut() } {
            if (*e).0.is_zero() && (*e).1.is_zero() {
                *e = (address, size);
                return true;
            }
        }
        false
    }
}
