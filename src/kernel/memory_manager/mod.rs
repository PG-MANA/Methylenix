/*
 * Memory Manager
 * This manager is the frontend of physical memory manager and page manager.
 */

pub mod kernel_malloc_manager;
pub mod physical_memory_manager;
pub mod pool_allocator;
/* pub mod reverse_memory_map_manager; */
pub mod virtual_memory_manager;

use arch::target_arch::paging::{PAGE_MASK, PAGE_SHIFT, PAGE_SIZE};

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

#[derive(Clone, Eq, PartialEq, Copy, Debug)]
pub enum MemoryError {
    SizeNotAligned,
    InvalidSize,
    AddressNotAligned,
    AllocPhysicalAddressFailed,
    FreeAddressFailed,
    InvalidPhysicalAddress,
    MapAddressFailed,
    InvalidVirtualAddress,
    InsertEntryFailed,
    AddressNotAvailable,
    PagingError,
    MutexError,
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
    ) -> Result<usize, MemoryError> {
        /* ADD: lazy allocation */
        /* return physically continuous 2 ^ order pages memory. */
        let size = Self::index_to_offset(1 << order);
        let mut physical_memory_manager = self.physical_memory_manager.lock().unwrap();
        if let Some(physical_address) = physical_memory_manager.alloc(size, true) {
            match self.virtual_memory_manager.alloc_address(
                size,
                physical_address,
                permission,
                &mut physical_memory_manager,
            ) {
                Ok(address) => {
                    self.virtual_memory_manager.update_paging(address);
                    Ok(address)
                }
                Err(e) => {
                    physical_memory_manager.free(physical_address, size, false);
                    Err(e)
                }
            }
        } else {
            Err(MemoryError::AllocPhysicalAddressFailed)
        }
    }

    pub fn alloc_nonlinear_pages(
        &mut self,
        _order: usize,
        _permission: MemoryPermissionFlags,
    ) -> Result<usize, MemoryError> {
        /* THINK: rename*/
        /* vmalloc */
        unimplemented!();
    }

    pub fn free(&mut self, vm_address: usize) -> Result<(), MemoryError> {
        let mut pm_manager = self.physical_memory_manager.lock().unwrap();
        let aligned_vm_address = vm_address & PAGE_MASK;
        if let Err(e) = self
            .virtual_memory_manager
            .free_address(aligned_vm_address, &mut pm_manager)
        {
            pr_err!("{:?}", e); /* free's error tends to be ignored. */
            Err(e)
        } else {
            Ok(())
        }
        /* Freeing Physical Memory will be done by Virtual Memory Manager, if it be needed. */
    }

    pub fn free_physical_memory(&mut self, physical_address: usize, size: usize) -> bool {
        /* initializing use only */
        if let Ok(mut pm_manager) = self.physical_memory_manager.try_lock() {
            pm_manager.free(physical_address, size, false)
        } else {
            false
        }
    }

    pub fn mmap_dev(
        &mut self,
        physical_address: usize,
        size: usize,
        permission: MemoryPermissionFlags,
    ) -> Result<usize, MemoryError> {
        /* for io_map */
        /* should remake... */
        let (aligned_physical_address, aligned_size) = Self::page_align(physical_address, size);
        let mut pm_manager = if let Ok(p) = self.physical_memory_manager.try_lock() {
            p
        } else {
            /* add: maybe sleep option */
            return Err(MemoryError::MutexError);
        };

        //pm_manager.reserve_memory(aligned_physical_address, size, false);
        // assume: physical_address must be reserved.
        /* add: check succeeded or failed (failed because of already reserved is ok, but other... )*/
        let virtual_address = self.virtual_memory_manager.mmap_dev(
            aligned_physical_address,
            None,
            aligned_size,
            permission,
            &mut pm_manager,
        )?;
        Ok(virtual_address + physical_address - aligned_physical_address)
    }

    pub fn mremap_dev(
        &mut self,
        old_virtual_address: usize,
        _old_size: usize,
        new_size: usize,
    ) -> Result<usize, MemoryError> {
        let (aligned_virtual_address, aligned_new_size) =
            Self::page_align(old_virtual_address, new_size);

        let mut pm_manager = if let Ok(p) = self.physical_memory_manager.try_lock() {
            p
        } else {
            /* add: maybe sleep option */
            return Err(MemoryError::MutexError);
        };

        //pm_manager.reserve_memory(aligned_physical_address, size, false);
        // assume: physical_address must be reserved.
        /* add: check succeeded or failed (failed because of already reserved is ok, but other... )*/

        let new_virtual_address = self.virtual_memory_manager.resize_memory_mapping(
            aligned_virtual_address,
            aligned_new_size,
            &mut pm_manager,
        )?;
        Ok(new_virtual_address + (old_virtual_address - aligned_virtual_address))
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

    pub const fn page_align(address: usize, size: usize) -> (usize /*address*/, usize /*size*/) {
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
        let mut page_count = (((size - 1) & PAGE_MASK) >> PAGE_SHIFT) + 1;
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

    pub const fn offset_to_index(offset: usize) -> usize {
        offset >> PAGE_SHIFT
    }

    pub const fn index_to_offset(index: usize) -> usize {
        use core::usize;
        assert!(index <= Self::offset_to_index(usize::MAX));
        index << PAGE_SHIFT
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
    pub const DEV_MAP: u16 = 1 << 3; /* マップしている物理メモリはなにか意味がある */
    pub const DIRECT_MAP: u16 = 1 << 4;

    pub const fn new(flags: u16) -> Self {
        if flags & (!0x1F) != 0 {
            /* when you add option, you must change this assert */
            panic!("Invalid flags are set.");
            /*static_assert*/
        }
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

    pub fn is_dev_map(&self) -> bool {
        self.flags & Self::DEV_MAP != 0
    }

    pub fn is_direct_mapped(&self) -> bool {
        self.flags & Self::DIRECT_MAP != 0
    }
}
