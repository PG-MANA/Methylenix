/*
 * Memory Manager
 * This manager is the frontend of physical memory manager and page manager.
 */

pub mod kernel_malloc_manager;
pub mod physical_memory_manager;
/* pub mod reverse_memory_map_manager; */
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

#[derive(Clone, Eq, PartialEq, Copy)]
pub struct MemoryOptionFlags {
    flags: u16,
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
        permission: MemoryPermissionFlags,
    ) -> Option<usize> {
        /* ADD: lazy allocation */
        /* return physically continuous 2 ^ order pages memory. */
        let size = PAGE_SIZE * (1 << order);
        let mut physical_memory_manager = self.physical_memory_manager.lock().unwrap();
        if let Some(physical_address) = physical_memory_manager.alloc(size, true) {
            if let Some(address) = self.virtual_memory_manager.alloc_address(
                size,
                physical_address,
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
        permission: MemoryPermissionFlags,
    ) -> Option<usize> {
        /* THINK: rename*/
        /* vmalloc */
        /* vfreeの際に全てのメモリが開放されないバグを含んでいる */
        if order == 0 {
            return self.alloc_pages(order, permission);
        }
        let count = 1 << order;
        let size = PAGE_SIZE * count;
        let address = if let Some(a) = self.virtual_memory_manager.get_free_address(size) {
            a
        } else {
            return None;
        };
        let mut pm_manager = self.physical_memory_manager.lock().unwrap();
        for i in 0..count {
            if let Some(physical_address) = pm_manager.alloc(PAGE_SIZE, true) {
                if self
                    .virtual_memory_manager
                    .map_address(
                        physical_address,
                        Some(address + i * PAGE_SIZE),
                        PAGE_SIZE,
                        permission,
                        MemoryOptionFlags::new(MemoryOptionFlags::NORMAL),
                        &mut pm_manager,
                    )
                    .is_err()
                {
                    for j in 0..i {
                        self.virtual_memory_manager
                            .free_address(address + j * PAGE_SIZE, &mut pm_manager);
                        self.virtual_memory_manager
                            .update_paging(address + j * PAGE_SIZE);
                    }
                    return None;
                }
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

    pub fn memory_remap(
        &mut self,
        physical_address: usize,
        size: usize,
        permission: MemoryPermissionFlags,
        flags: MemoryOptionFlags,
    ) -> Result<usize, &str> {
        /* for io_map */
        /* should remake... */
        let (aligned_physical_address, aligned_size) = Self::page_round_up(physical_address, size);
        let pm_manager = self.physical_memory_manager.try_lock();
        if pm_manager.is_err() {
            /* add: maybe sleep option */
            return Err("Cannot lock physical_memory_manager");
        };
        let mut pm_manager = pm_manager.unwrap();
        pm_manager.reserve_memory(aligned_physical_address, size, false);
        /* add: check succeeded or failed (failed because of already reserved is ok, but other... )*/
        let virtual_address = self.virtual_memory_manager.map_address(
            aligned_physical_address,
            None,
            aligned_size,
            permission,
            flags,
            &mut pm_manager,
        )?;
        Ok(virtual_address + physical_address - aligned_physical_address)
    }

    pub fn resize_memory_remap(
        &mut self,
        virtual_address: usize,
        new_size: usize,
    ) -> Result<usize, &str> {
        let (aligned_virtual_address, aligned_new_size) =
            Self::page_round_up(virtual_address, new_size);

        let mut pm_manager = if let Ok(p) = self.physical_memory_manager.try_lock() {
            p
        } else {
            /* add: maybe sleep option */
            return Err("Cannot lock physical_memory_manager");
        };
        let physical_address = if let Some(a) = self
            .virtual_memory_manager
            .virtual_address_to_physical_address(aligned_virtual_address)
        /* may be slow*/
        {
            a
        } else {
            return Err("Invalid virtual address");
        };
        pm_manager.reserve_memory(physical_address, new_size, false);
        /* add: check succeeded or failed (failed because of already reserved is ok, but other... )*/
        if !self.virtual_memory_manager.try_expand_size(
            aligned_virtual_address,
            aligned_new_size,
            &mut pm_manager,
        ) {
            let new_virtual_address = self.virtual_memory_manager.resize_memory_mapping(
                aligned_virtual_address,
                aligned_new_size,
                &mut pm_manager,
            )?;
            Ok(new_virtual_address + virtual_address - aligned_virtual_address)
        } else {
            Ok(virtual_address)
        }
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

impl MemoryPermissionFlags {
    /* Bitfield代わりとして使っているので命名規則は変えている。 */
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

impl MemoryOptionFlags {
    /* Bitfield代わりとして使っているので命名規則は変えている。 */
    pub const NORMAL: u16 = 0;
    pub const PRE_RESERVED: u16 = 1 << 0;
    pub const DO_NOT_FREE_PHY_ADDR: u16 = 1 << 1;
    pub const WIRED: u16 = 1 << 2;
    pub const DIRECT_MAP: u16 = 1 << 3;

    pub fn new(flags: u16) -> Self {
        assert_eq!(flags & !0xF, 0);
        Self { flags }
    }

    pub fn pre_reserved(&self) -> bool {
        self.flags & Self::PRE_RESERVED != 0
    }

    pub fn do_not_free_phy_addr(&self) -> bool {
        self.flags & Self::DO_NOT_FREE_PHY_ADDR != 0
    }

    pub fn wired(&self) -> bool {
        self.flags & Self::WIRED != 0
    }
}
