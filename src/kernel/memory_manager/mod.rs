/*
 * Memory Manager
 * This manager is the frontend of physical memory manager and page manager.
 * In this memory system, you should not use alloc::*, use only core::*
 */

pub mod data_type;
pub mod global_allocator;
pub mod object_allocator;
pub mod physical_memory_manager;
pub mod pool_allocator;
pub mod virtual_memory_manager;

use self::data_type::{Address, MOrder, MSize, PAddress, VAddress};
use self::physical_memory_manager::PhysicalMemoryManager;
use self::virtual_memory_manager::VirtualMemoryManager;

use crate::arch::target_arch::paging::{PAGE_MASK, PAGE_SHIFT, PAGE_SIZE};

use crate::kernel::sync::spin_lock::Mutex;

pub struct MemoryManager {
    physical_memory_manager: &'static Mutex<PhysicalMemoryManager>,
    virtual_memory_manager: VirtualMemoryManager,
}

/* To share PhysicalMemoryManager */
pub struct SystemMemoryManager {
    original_physical_memory_manager: Mutex<PhysicalMemoryManager>,
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

impl SystemMemoryManager {
    pub const fn new(physical_memory_manager: PhysicalMemoryManager) -> Self {
        Self {
            original_physical_memory_manager: Mutex::new(physical_memory_manager),
        }
    }

    pub fn create_new_memory_manager(
        &'static self,
        virtual_memory_manager: VirtualMemoryManager,
    ) -> MemoryManager {
        MemoryManager::new(
            &self.original_physical_memory_manager,
            virtual_memory_manager,
        )
    }
}

impl MemoryManager {
    pub fn new(
        physical_memory_manager: &'static Mutex<PhysicalMemoryManager>,
        virtual_memory_manager: VirtualMemoryManager,
    ) -> Self {
        /*カーネル領域の予約*/
        MemoryManager {
            physical_memory_manager,
            virtual_memory_manager,
        }
    }

    pub fn alloc_pages(
        &mut self,
        order: MOrder,
        permission: MemoryPermissionFlags,
    ) -> Result<VAddress, MemoryError> {
        /* ADD: lazy allocation */
        /* return physically continuous 2 ^ order pages memory. */
        let size = order.to_offset() << MSize::from(PAGE_SHIFT);
        let mut physical_memory_manager = self.physical_memory_manager.lock().unwrap();
        if let Some(physical_address) = physical_memory_manager.alloc(size, PAGE_SHIFT.into()) {
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
        order: MOrder,
        permission: MemoryPermissionFlags,
    ) -> Result<VAddress, MemoryError> {
        /* THINK: rename*/
        /* vmalloc */
        let size = order.to_offset() << MSize::from(PAGE_SHIFT);
        if size <= PAGE_SIZE {
            return self.alloc_pages(order, permission);
        }
        let pm_manager = self.physical_memory_manager.try_lock();
        if pm_manager.is_err() {
            return Err(MemoryError::MutexError);
        }
        let mut pm_manager = pm_manager.unwrap();
        let entry = self.virtual_memory_manager.alloc_address_without_mapping(
            size,
            permission,
            MemoryOptionFlags::new(MemoryOptionFlags::NORMAL),
            &mut pm_manager,
        )?;
        let vm_start_address = entry.get_vm_start_address();
        for i in 0.into()..size.to_index() {
            if let Some(physical_address) = pm_manager.alloc(PAGE_SIZE, PAGE_SHIFT.into()) {
                if let Err(e) = self
                    .virtual_memory_manager
                    .insert_physical_page_into_vm_map_entry(
                        entry,
                        vm_start_address + i.to_offset(),
                        physical_address,
                        &mut pm_manager,
                    )
                {
                    panic!("Cannot insert physical page into vm_entry Err:{:?}", e);
                }
            }
        }
        if let Err(e) = self
            .virtual_memory_manager
            .finalize_vm_map_entry(entry, &mut pm_manager)
        {
            panic!("Cannot finalize vm_map_entry Err:{:?}", e);
        }
        Ok(vm_start_address)
    }

    pub fn alloc_with_option(
        &mut self,
        order: MOrder,
        permission: MemoryPermissionFlags,
        option: MemoryOptionFlags,
    ) -> Result<VAddress, MemoryError> {
        let size = order.to_offset() << MSize::from(PAGE_SHIFT);

        if option.is_direct_mapped() && permission.execute() == false {
            let mut physical_memory_manager = self.physical_memory_manager.lock().unwrap();
            return self
                .virtual_memory_manager
                .alloc_from_direct_map(size, &mut physical_memory_manager);
        }
        unimplemented!()
    }

    pub fn free(&mut self, vm_address: VAddress) -> Result<(), MemoryError> {
        let mut pm_manager = self.physical_memory_manager.lock().unwrap();
        let aligned_vm_address = vm_address & PAGE_MASK;
        if let Err(e) = self
            .virtual_memory_manager
            .free_address(aligned_vm_address.into(), &mut pm_manager)
        {
            pr_err!("{:?}", e); /* free's error tends to be ignored. */
            Err(e)
        } else {
            Ok(())
        }
        /* Freeing Physical Memory will be done by Virtual Memory Manager, if it be needed. */
    }

    pub fn alloc_physical_memory(&mut self, order: MOrder) -> Result<PAddress, MemoryError> {
        /* initializing use only */
        /* returned memory area is not mapped, if you want to access, you must map. */
        let size = order.to_offset() << MSize::from(PAGE_SHIFT);
        let mut physical_memory_manager = self.physical_memory_manager.lock().unwrap();
        if let Some(physical_address) = physical_memory_manager.alloc(size, PAGE_SHIFT.into()) {
            Ok(physical_address)
        } else {
            Err(MemoryError::AllocPhysicalAddressFailed)
        }
    }

    pub fn free_physical_memory(&mut self, physical_address: PAddress, size: MSize) -> bool {
        /* initializing use only */
        if let Ok(mut pm_manager) = self.physical_memory_manager.try_lock() {
            pm_manager.free(physical_address, size, false)
        } else {
            false
        }
    }

    pub fn mmap(
        &mut self,
        physical_address: PAddress,
        size: MSize,
        permission: MemoryPermissionFlags,
        option: MemoryOptionFlags,
        should_reserve_physical_memory: bool,
    ) -> Result<VAddress, MemoryError> {
        /* for some data */
        /* should remake... */
        let (aligned_physical_address, aligned_size) = Self::page_align(physical_address, size);
        let mut pm_manager = if let Ok(p) = self.physical_memory_manager.try_lock() {
            p
        } else {
            /* add: maybe sleep option */
            return Err(MemoryError::MutexError);
        };

        if should_reserve_physical_memory {
            pm_manager.reserve_memory(aligned_physical_address, size, 0.into());
        }
        let virtual_address = self.virtual_memory_manager.map_address(
            aligned_physical_address,
            None,
            aligned_size,
            permission,
            option,
            &mut pm_manager,
        )?;
        Ok(virtual_address + (physical_address - aligned_physical_address))
    }

    pub fn mmap_dev(
        &mut self,
        physical_address: PAddress,
        size: MSize,
        permission: MemoryPermissionFlags,
    ) -> Result<VAddress, MemoryError> {
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
        Ok(virtual_address + (physical_address - aligned_physical_address))
    }

    pub fn mremap_dev(
        &mut self,
        old_virtual_address: VAddress,
        _old_size: MSize,
        new_size: MSize,
    ) -> Result<VAddress, MemoryError> {
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

    #[inline] /* want to be const... */
    pub fn page_align<T: Address>(address: T, size: MSize) -> (T /*address*/, MSize /*size*/) {
        if size.is_zero() && (address.to_usize() & PAGE_MASK) == 0 {
            (address, MSize::new(0))
        } else {
            (
                (address.to_usize() & PAGE_MASK).into(),
                MSize::new(
                    (size.to_usize() + (address.to_usize() - (address.to_usize() & PAGE_MASK)) - 1)
                        & PAGE_MASK,
                ) + PAGE_SIZE,
            )
        }
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
