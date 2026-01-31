//!
//! Paging Manager
//!
//! These modules treat the paging system of AArch64.
//!
//! This does not handle memory status(which process using what memory area).
//! This is the back-end of VirtualMemoryManager.

mod table_entry;

use self::table_entry::{NUM_OF_TABLE_ENTRIES, PageTableEntry};

use crate::arch::target_arch::context::memory_layout::{
    direct_map_to_physical_address, get_high_memory_base_address, physical_address_to_direct_map,
};
use crate::arch::target_arch::device::cpu;

use crate::kernel::memory_manager::data_type::*;
use crate::kernel::memory_manager::physical_memory_manager::PhysicalMemoryManager;

use core::slice::from_raw_parts_mut;

/// Default Page Size, the mainly using 4KiB paging.(Type = MSize)
pub const PAGE_SIZE: MSize = MSize::new(PAGE_SIZE_USIZE);

/// Default Page Size, the mainly using 4KiB paging.(Type = usize)
pub const PAGE_SIZE_USIZE: usize = 0x1000;

/// PAGE_SIZE = 1 << PAGE_SHIFT(Type = usize)
pub const PAGE_SHIFT: usize = 12;

/// if !PAGE_MASK & address !=0 => address is not page aligned.
pub const PAGE_MASK: usize = !0xFFF;

/// Default page cache size for paging
pub const PAGING_CACHE_LENGTH: usize = 64;

/// Max virtual address of AArch64(Type = VAddress)
pub const MAX_VIRTUAL_ADDRESS: VAddress = VAddress::new(MAX_VIRTUAL_ADDRESS_USIZE);

/// Max virtual address of AArch64(Type = usize)
pub const MAX_VIRTUAL_ADDRESS_USIZE: usize = 0xFFFF_FFFF_FFFF_FFFF;

pub const NEED_COPY_HIGH_MEMORY_PAGE_TABLE: bool = true;

/// PageManager
///
/// This controls paging system.
/// This manager does not check if specified address is usable,
/// that should be done by VirtualMemoryManager.
#[derive(Clone)]
pub struct PageManager {
    page_table: VAddress,
    asid: u16,
    mode: u8,
}

/// Paging Error enum
///
/// This enum is used to pass error from PageManager.
#[derive(Clone, Eq, PartialEq, Copy, Debug)]
pub enum PagingError {
    MemoryCacheRanOut,
    MemoryCacheOverflowed,
    EntryIsNotFound,
    AddressIsNotAligned,
    AddressIsNotCanonical,
    SizeIsNotAligned,
    InvalidPageTable,
    MemoryError,
}

impl PageManager {
    /// Create System Page Manager
    /// Before use, **you must call [`Self::init`]**.
    pub const fn new() -> Self {
        Self {
            page_table: VAddress::new(0),
            asid: 0,
            mode: 0,
        }
    }

    /// Init PageManager
    ///
    /// This function must be called only once on boot time.
    pub fn init(&mut self, pm_manager: &mut PhysicalMemoryManager) -> Result<(), PagingError> {
        self.page_table = Self::alloc_page_table(pm_manager)?;
        self.mode = 9; // TODO: set dynamically

        Ok(())
    }

    pub fn init_user(
        &mut self,
        system_page_manager: &Self,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), PagingError> {
        self.page_table = Self::alloc_page_table(pm_manager)?;
        self.mode = system_page_manager.mode;
        self.asid = system_page_manager.asid; /* TODO: set dynamically */
        for e in self
            .get_table_and_initial_level(VAddress::new(0))?
            .0
            .iter_mut()
        {
            e.init();
        }
        self.copy_system_area(system_page_manager)?;
        Ok(())
    }

    pub fn copy_system_area(&mut self, system_page_manager: &Self) -> Result<(), PagingError> {
        let high_memory_address = get_high_memory_base_address();
        let (table, level) = self.get_table_and_initial_level(high_memory_address)?;
        let high_area_start = (high_memory_address.to_usize() >> (PAGE_SHIFT + 9 * level as usize))
            & (table.len() - 1);
        let (system_table, system_level) =
            system_page_manager.get_table_and_initial_level(high_memory_address)?;
        if level != system_level {
            return Err(PagingError::InvalidPageTable);
        }
        for (e, o) in table[high_area_start..]
            .iter_mut()
            .zip(system_table[high_area_start..].iter())
        {
            *e = o.clone();
        }
        Ok(())
    }

    pub fn get_canonical_address(address: VAddress) -> Result<VAddress, PagingError> {
        Ok(address)
    }

    fn get_table_and_initial_level(
        &self,
        _virtual_address: VAddress,
    ) -> Result<(&'static mut [PageTableEntry], u8), PagingError> {
        if self.mode < 8 || self.mode > 10 {
            return Err(PagingError::InvalidPageTable);
        }
        let level = self.mode - 8 + 3;

        Ok((
            unsafe {
                from_raw_parts_mut(
                    self.page_table.to_usize() as *mut PageTableEntry,
                    NUM_OF_TABLE_ENTRIES,
                )
            },
            level,
        ))
    }

    fn get_target_entry<'a>(
        &self,
        table: &'a mut [PageTableEntry],
        level: u8,
        virtual_address: VAddress,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(&'a mut PageTableEntry, MSize), PagingError> {
        let index =
            (virtual_address.to_usize() >> (PAGE_SHIFT + 9 * level as usize)) & (table.len() - 1);

        if table[index].is_leaf() {
            Ok((
                &mut table[index],
                MSize::new(1usize << (PAGE_SHIFT + 9 * (level as usize))),
            ))
        } else if table[index].has_next() {
            assert_ne!(level, 0);
            self.get_target_entry(
                unsafe {
                    from_raw_parts_mut(
                        physical_address_to_direct_map(table[index].get_next_table_address())
                            .to_usize() as *mut _,
                        NUM_OF_TABLE_ENTRIES,
                    )
                },
                level - 1,
                virtual_address,
                pm_manager,
            )
        } else {
            Err(PagingError::EntryIsNotFound)
        }
    }

    fn set_permission_and_options(
        e: &mut PageTableEntry,
        p: MemoryPermissionFlags,
        o: MemoryOptionFlags,
    ) {
        e.set_permission(p);
        e.set_permission(p);
        if o.is_device_memory() || o.is_io_map() {
            /* todo: */
        } else {
            /* todo: */
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn _associate_address(
        &self,
        table: &mut [PageTableEntry],
        level: u8,
        pm_manager: &mut PhysicalMemoryManager,
        physical_address: &mut PAddress,
        virtual_address: &mut VAddress,
        size: &mut MSize,
        permission: MemoryPermissionFlags,
        option: MemoryOptionFlags,
        remove: bool,
    ) -> Result<(), PagingError> {
        let index =
            (virtual_address.to_usize() >> (PAGE_SHIFT + 9 * level as usize)) & (table.len() - 1);
        let block_size = MSize::new(1 << (PAGE_SHIFT + 9 * level as usize));
        for e in table[index..].iter_mut() {
            if size.is_zero() {
                break;
            }
            if ((*physical_address & (block_size.to_usize() - 1)) == 0) && (*size >= block_size) {
                /* Leaf Entry */
                e.init();
                if !remove {
                    Self::set_permission_and_options(e, permission, option);
                    e.validate();
                }
                e.set_output_address(*physical_address);
                *size -= block_size;
                *physical_address += block_size;
                *virtual_address += block_size;
            } else {
                assert_ne!(level, 0);
                let next_table_address;
                let mut created = false;
                if !e.has_next() {
                    if remove {
                        return Err(PagingError::EntryIsNotFound);
                    }
                    next_table_address = Self::alloc_page_table(pm_manager)?;
                    created = true;
                } else {
                    next_table_address = physical_address_to_direct_map(e.get_next_table_address());
                }
                let result = self._associate_address(
                    unsafe {
                        from_raw_parts_mut(
                            next_table_address.to_usize() as *mut _,
                            NUM_OF_TABLE_ENTRIES,
                        )
                    },
                    level - 1,
                    pm_manager,
                    physical_address,
                    virtual_address,
                    size,
                    permission,
                    option,
                    remove,
                );
                if created {
                    if result.is_ok() {
                        *e = PageTableEntry::create_table_entry(direct_map_to_physical_address(
                            next_table_address,
                        ));
                    } else {
                        let _ = Self::free_page_table(pm_manager, next_table_address);
                    }
                }
                result?;
            }
        }
        Ok(())
    }

    /// Map virtual_address to physical address with size.
    ///
    /// This function will map from virtual_address to virtual_address + size.
    /// This function is used to map consecutive physical address.
    /// This may use 2MB or 1GB paging when [`MemoryOptionFlags::should_use_huge_page`] or
    /// [`MemoryOptionFlags::is_device_memory`] or [`MemoryOptionFlags::is_io_map`] is true.
    /// This function does not flush page table and invoke page cache. You should do them manually.
    pub fn associate_address(
        &self,
        pm_manager: &mut PhysicalMemoryManager,
        mut physical_address: PAddress,
        virtual_address: VAddress,
        mut size: MSize,
        permission: MemoryPermissionFlags,
        option: MemoryOptionFlags,
    ) -> Result<(), PagingError> {
        if ((physical_address.to_usize() & !PAGE_MASK) != 0)
            || ((virtual_address.to_usize() & !PAGE_MASK) != 0)
        {
            return Err(PagingError::AddressIsNotAligned);
        } else if (size.to_usize() & !PAGE_MASK) != 0 {
            return Err(PagingError::SizeIsNotAligned);
        }
        let (table, level) = self.get_table_and_initial_level(virtual_address)?;

        self._associate_address(
            table,
            level,
            pm_manager,
            &mut physical_address,
            &mut Self::get_canonical_address(virtual_address)?,
            &mut size,
            permission,
            option,
            false,
        )?;
        if !size.is_zero() {
            Err(PagingError::EntryIsNotFound)
        } else {
            cpu::flush_data_cache_all();
            Ok(())
        }
    }

    /// Change permission of virtual_address
    ///
    /// This function searches the target page table entry and changes the permission.
    /// If virtual_address is not valid, this will return PagingError::EntryIsNotFound.
    pub fn change_memory_permission(
        &self,
        pm_manager: &mut PhysicalMemoryManager,
        virtual_address: VAddress,
        size: MSize,
        permission: MemoryPermissionFlags,
        option: MemoryOptionFlags,
    ) -> Result<(), PagingError> {
        if (virtual_address.to_usize() & !PAGE_MASK) != 0 {
            return Err(PagingError::AddressIsNotAligned);
        } else if (size.to_usize() & !PAGE_MASK) != 0 {
            return Err(PagingError::SizeIsNotAligned);
        }
        let mut s = MSize::new(0);
        let (table, level) = self.get_table_and_initial_level(virtual_address)?;

        while s != size {
            let (entry, t) = self.get_target_entry(table, level, virtual_address, pm_manager)?;
            if s + t > size {
                return Err(PagingError::InvalidPageTable);
            }
            Self::set_permission_and_options(entry, permission, option);
            s += t;
        }
        cpu::flush_data_cache_all();
        Ok(())
    }

    /// Unmap virtual_address ~ (virtual_address + size)
    ///
    /// This function searches target page entries and disable present flag.
    /// After disabling, this calls [`Self::cleanup_page_table`] to collect freed page tables.
    /// If target entry does not exist, this function will return [`PagingError::EntryIsNotFound`].
    /// When a huge table was used and the mapped size is different from expected size, this will return error.
    pub fn unassociate_address(
        &self,
        virtual_address: VAddress,
        mut size: MSize,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), PagingError> {
        if (virtual_address.to_usize() & !PAGE_MASK) != 0 {
            return Err(PagingError::AddressIsNotAligned);
        } else if (size.to_usize() & !PAGE_MASK) != 0 {
            return Err(PagingError::SizeIsNotAligned);
        }

        let virtual_address = Self::get_canonical_address(virtual_address)?;
        let (table, level) = self.get_table_and_initial_level(virtual_address)?;
        let mut v = virtual_address;
        self._associate_address(
            table,
            level,
            pm_manager,
            &mut PAddress::new(0),
            &mut v,
            &mut size,
            MemoryPermissionFlags::rodata(),
            MemoryOptionFlags::KERNEL,
            true,
        )?;

        cpu::flush_data_cache_all();
        if !size.is_zero() {
            return Err(PagingError::InvalidPageTable);
        }
        if self._cleanup_page_tables(table, level, virtual_address, pm_manager)? {
            Err(PagingError::InvalidPageTable)
        } else {
            Ok(())
        }
    }

    fn _cleanup_page_tables(
        &self,
        table: &mut [PageTableEntry],
        level: u8,
        virtual_address: VAddress,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<bool, PagingError> {
        if level == 0 {
            return Ok(!table.iter().any(|e| e.is_valid()));
        }
        let index =
            (virtual_address.to_usize() >> (PAGE_SHIFT + 9 * level as usize)) & (table.len() - 1);
        if table[index].has_next() {
            let next_table_address = table[index].get_next_table_address();
            if !self._cleanup_page_tables(
                unsafe {
                    from_raw_parts_mut(
                        next_table_address.to_usize() as *mut _,
                        NUM_OF_TABLE_ENTRIES,
                    )
                },
                level - 1,
                virtual_address,
                pm_manager,
            )? {
                return Ok(false);
            }
            table[index].invalidate();
            /* Free this table */
            pm_manager
                .free(next_table_address, PAGE_SIZE, false)
                .map_err(|e| {
                    pr_warn!("Failed to free the page table: {e:?}");
                    PagingError::MemoryError
                })?;
        }
        return Ok(!table.iter().any(|e| e.is_valid()));
    }

    /// Clean up the page table.
    pub fn cleanup_page_table(
        &self,
        virtual_address: VAddress,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), PagingError> {
        let (table, level) = self.get_table_and_initial_level(virtual_address)?;
        if self._cleanup_page_tables(
            table,
            level,
            Self::get_canonical_address(virtual_address)?,
            pm_manager,
        )? {
            Err(PagingError::InvalidPageTable)
        } else {
            cpu::flush_data_cache_all();
            // TODO: flush TLB
            Ok(())
        }
    }

    pub fn destroy_page_table(
        &mut self,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), PagingError> {
        /* TODO: clean up all PTEs expect the kernel ones */
        let table = self.page_table;
        Self::free_page_table(pm_manager, table)?;
        *self = Self::new();
        Ok(())
    }

    /// Allocate the page table.
    fn alloc_page_table(pm_manager: &mut PhysicalMemoryManager) -> Result<VAddress, PagingError> {
        match pm_manager.alloc(PAGE_SIZE, MOrder::new(PAGE_SHIFT)) {
            Ok(p) => {
                let table_address = physical_address_to_direct_map(p);
                let table =
                    unsafe { &mut *(table_address.to::<[PageTableEntry; NUM_OF_TABLE_ENTRIES]>()) };
                for e in table {
                    *e = PageTableEntry::new();
                }
                cpu::flush_data_cache_all();
                Ok(table_address)
            }
            Err(_) => Err(PagingError::MemoryCacheRanOut),
        }
    }

    fn free_page_table(
        pm_manager: &mut PhysicalMemoryManager,
        table_address: VAddress,
    ) -> Result<(), PagingError> {
        pm_manager
            .free(
                direct_map_to_physical_address(table_address),
                PAGE_SIZE,
                false,
            )
            .map_err(|e| {
                pr_warn!("Failed to free the page table: {e:?}");
                PagingError::MemoryError
            })
    }

    /// Flush page table and apply new page table.
    pub fn flush_page_table(&mut self) {
        cpu::flush_data_cache_all();
        const SATP_MODE_OFFSET: usize = 60;
        const SATP_ASID_OFFSET: usize = 44;
        let satp = ((self.mode as u64) << SATP_MODE_OFFSET)
            | ((self.asid as u64) << SATP_ASID_OFFSET)
            | (direct_map_to_physical_address(self.page_table).to_usize() >> PAGE_SHIFT) as u64;
        unsafe { cpu::set_satp(satp) };
        // TODO: flush TLB
    }

    /// Delete the paging cache of the target address and update it.
    ///
    /// Currently, RISC-V does not have any TLB maintenance instructions.
    /// So, only flush data cache.
    pub fn update_page_cache(_virtual_address: VAddress, _range: MSize) {
        Self::update_page_cache_all()
    }

    /// Delete all TLBs
    ///
    /// Currently, RISC-V does not have any TLB maintenance instructions.
    /// So, only flush data cache.
    pub fn update_page_cache_all() {
        cpu::flush_data_cache_all();
        // TODO: flush TLB
    }

    fn _dump_table(
        &self,
        start_address: VAddress,
        end_address: VAddress,
        table: &mut [PageTableEntry],
        virtual_address: &mut VAddress,
        level: u8,
        continued_address: &mut (VAddress, PAddress, MemoryPermissionFlags),
        omitted: &mut bool,
    ) {
        let print_normal = |v: VAddress, p: PAddress, pm: MemoryPermissionFlags| {
            kprintln!(
                "VA: {:>#16X} => PA: {:>#16X}, W:{:>5}, E:{:>5}, U:{:>5}",
                v.to_usize(),
                p.to_usize(),
                pm.is_writable(),
                pm.is_executable(),
                pm.is_user_accessible()
            );
        };
        let print_omitted = |v: VAddress, p: PAddress| {
            kprintln!(
                "... {:>#16X}        {:>#16X} (fin)",
                v.to_usize(),
                p.to_usize()
            );
        };
        let block_size = MSize::new(1 << (PAGE_SHIFT + 9 * level as usize));
        for e in table {
            if *virtual_address >= end_address {
                return;
            } else if *virtual_address < start_address {
                *virtual_address += block_size;
                continue;
            }
            if !e.is_valid() {
                *virtual_address += block_size;
                if *omitted {
                    print_omitted(continued_address.0, continued_address.1);
                    *omitted = false;
                }
                continue;
            }
            if e.is_leaf() {
                let p = e.get_permission();
                let pa = e.get_output_address();
                if *omitted {
                    if *virtual_address != continued_address.0
                        || pa != continued_address.1
                        || p != continued_address.2
                    {
                        print_omitted(continued_address.0, continued_address.1);
                        *omitted = false;
                    } else {
                        continued_address.0 += block_size;
                        continued_address.1 += block_size;
                        *virtual_address += block_size;
                        continue;
                    }
                } else if *virtual_address == continued_address.0
                    && pa == continued_address.1
                    && p == continued_address.2
                {
                    continued_address.0 += block_size;
                    continued_address.1 += block_size;
                    *virtual_address += block_size;
                    *omitted = true;
                    continue;
                } else {
                    print_normal(*virtual_address, pa, p);
                    *virtual_address += block_size;
                    continued_address.0 = *virtual_address;
                    continued_address.1 = pa + block_size;
                    continued_address.2 = p;
                }
            } else if e.has_next() {
                self._dump_table(
                    start_address,
                    end_address,
                    unsafe {
                        from_raw_parts_mut(
                            physical_address_to_direct_map(e.get_next_table_address())
                                .to::<PageTableEntry>(),
                            NUM_OF_TABLE_ENTRIES,
                        )
                    },
                    virtual_address,
                    level - 1,
                    continued_address,
                    omitted,
                );
            }
        }
    }

    /// Dump paging table
    ///
    /// This function shows the status of paging, it prints a lot.
    pub fn dump_table(&self, start: Option<VAddress>, end: Option<VAddress>) {
        let print_omitted = |v: VAddress, p: PAddress| {
            kprintln!(
                "... {:>#16X}        {:>#16X} (end)",
                v.to_usize(),
                p.to_usize()
            );
        };
        let start = start
            .map(|a| Self::get_canonical_address(a).unwrap_or(VAddress::new(0)))
            .unwrap_or(VAddress::new(0));
        let end = end
            .map(|a| Self::get_canonical_address(a).unwrap_or(MAX_VIRTUAL_ADDRESS))
            .unwrap_or(MAX_VIRTUAL_ADDRESS);
        let Ok((table, level)) = self.get_table_and_initial_level(start) else {
            return;
        };
        let mut omitted = false;
        let mut continued_address = (
            VAddress::new(0),
            PAddress::new(0),
            MemoryPermissionFlags::new(false, false, false, false),
        );

        self._dump_table(
            start,
            end,
            table,
            &mut VAddress::new(0),
            level,
            &mut continued_address,
            &mut omitted,
        );
        if omitted {
            print_omitted(continued_address.0, continued_address.1);
        }
    }
}
