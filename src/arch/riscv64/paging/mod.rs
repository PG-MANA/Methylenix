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
    DIRECT_MAP_START_ADDRESS, HIGH_MEMORY_START_ADDRESS, direct_map_to_physical_address,
    physical_address_to_direct_map,
};
use crate::arch::target_arch::device::cpu;

use crate::kernel::memory_manager::MemoryError;
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
    MemoryError(MemoryError),
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
        /* TODO: Setup TCR_EL1 from scratch */
        let tcr_el1 = cpu::get_tcr();
        let t1sz = (tcr_el1 & cpu::TCR_EL1_T1SZ) >> cpu::TCR_EL1_T1SZ_OFFSET;

        /* Set memory address information */
        unsafe {
            HIGH_MEMORY_START_ADDRESS = VAddress::new(((1 << t1sz) - 1) << (64 - t1sz));
            DIRECT_MAP_START_ADDRESS = HIGH_MEMORY_START_ADDRESS;
        }

        /* Set up the page table */
        todo!();
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
        let high_memory_address = unsafe { HIGH_MEMORY_START_ADDRESS };
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

    const fn get_table_and_initial_level(
        &self,
        _virtual_address: VAddress,
    ) -> Result<(&mut [PageTableEntry], u8), PagingError> {
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

    fn _get_target_leaf_entry(
        &self,
        pm_manager: &mut PhysicalMemoryManager,
        virtual_address: VAddress,
        table: &mut [PageTableEntry],
        level: u8,
        should_create_entry: bool,
    ) -> Result<&mut PageTableEntry, PagingError> {
        let index =
            (virtual_address.to_usize() >> (PAGE_SHIFT + 9 * level as usize)) & (table.len() - 1);
        if level == 0 {
            return Ok(&mut table[index]);
        }

        if !table[index].has_next() {
            if table[index].is_leaf() || !should_create_entry {
                return Err(PagingError::EntryIsNotFound);
            }
            let new_table_address = Self::alloc_page_table(pm_manager)?;
            let result = self._get_target_leaf_entry(
                pm_manager,
                virtual_address,
                unsafe {
                    from_raw_parts_mut(virtual_address.to_usize() as *mut _, NUM_OF_TABLE_ENTRIES)
                },
                level - 1,
                should_create_entry,
            );
            if result.is_ok() {
                table[index] = PageTableEntry::create_table_entry(direct_map_to_physical_address(
                    new_table_address,
                ));
            } else {
                let _ = Self::free_page_table(pm_manager, new_table_address);
            }
            result
        } else {
            self._get_target_leaf_entry(
                pm_manager,
                virtual_address,
                unsafe {
                    from_raw_parts_mut(
                        physical_address_to_direct_map(table[index].get_next_table_address())
                            .to_usize() as *mut _,
                        NUM_OF_TABLE_ENTRIES,
                    )
                },
                level - 1,
                should_create_entry,
            )
        }
    }

    fn get_target_leaf_entry(
        &self,
        pm_manager: &mut PhysicalMemoryManager,
        virtual_address: VAddress,
        should_create_entry: bool,
    ) -> Result<&'static mut PageTableEntry, PagingError> {
        let (table, level) = self.get_table_and_initial_level(virtual_address)?;
        self._get_target_leaf_entry(
            pm_manager,
            virtual_address,
            table,
            level,
            should_create_entry,
        )
    }

    fn _get_target_entry(
        &self,
        pm_manager: &mut PhysicalMemoryManager,
        virtual_address: VAddress,
        table: &mut [PageTableEntry],
        level: u8,
    ) -> Result<&mut PageTableEntry, PagingError> {
        if level == 0 {
            return self._get_target_leaf_entry(pm_manager, virtual_address, table, level, false);
        }
        let index =
            (virtual_address.to_usize() >> (PAGE_SHIFT + 9 * level as usize)) & (table.len() - 1);

        if table[index].is_leaf() {
            Ok(&mut table[index])
        } else if table[index].has_next() {
            self._get_target_entry(
                pm_manager,
                virtual_address,
                unsafe {
                    from_raw_parts_mut(
                        physical_address_to_direct_map(table[index].get_next_table_address())
                            .to_usize() as *mut _,
                        NUM_OF_TABLE_ENTRIES,
                    )
                },
                level - 1,
            )
        } else {
            Err(PagingError::EntryIsNotFound)
        }
    }

    fn get_target_entry(
        &self,
        pm_manager: &mut PhysicalMemoryManager,
        virtual_address: VAddress,
    ) -> Result<&'static mut PageTableEntry, PagingError> {
        let (table, level) = self.get_table_and_initial_level(virtual_address)?;
        self._get_target_entry(pm_manager, virtual_address, table, level)
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

    /// Associate physical address with virtual_address.
    ///
    /// This function will get target page table entry from virtual_address
    /// (if not exist, will make) and set physical address.
    /// "Permission" will be used when set the page table entry attribute.
    /// If you want to associate wide area (except physical address is non-linear),
    /// you should use [`Self::associate_area`]. (it may use 2MB paging).
    ///
    /// This function does not flush page table and invoke page cache. You should do them manually.
    pub fn associate_address(
        &self,
        pm_manager: &mut PhysicalMemoryManager,
        physical_address: PAddress,
        virtual_address: VAddress,
        permission: MemoryPermissionFlags,
        option: MemoryOptionFlags,
    ) -> Result<(), PagingError> {
        if ((physical_address.to_usize() & !PAGE_MASK) != 0)
            || ((virtual_address.to_usize() & !PAGE_MASK) != 0)
        {
            return Err(PagingError::AddressIsNotAligned);
        }

        let entry = self.get_target_leaf_entry(pm_manager, virtual_address, true)?;
        entry.init();
        entry.set_output_address(physical_address);
        Self::set_permission_and_options(entry, permission, option);
        entry.validate();
        cpu::flush_data_cache_all();
        Ok(())
    }

    fn _associate_area(
        &self,
        level: u8,
        table: &mut [PageTableEntry],
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
            if *size.is_zero() {
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
                } else {
                    next_table_address = physical_address_to_direct_map(e.get_next_table_address());
                }
                let result = self._associate_area(
                    level - 1,
                    unsafe {
                        from_raw_parts_mut(
                            next_table_address.to_usize() as *mut _,
                            NUM_OF_TABLE_ENTRIES,
                        )
                    },
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
    /// This may use 2MB or 1GB paging.
    /// If you want to map non-consecutive physical address,
    /// you should call [`Self::associate_address`] repeatedly.
    ///
    /// This function does not flush page table and invoke page cache. You should do them manually.
    pub fn associate_area(
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

        self._associate_area(
            level,
            table,
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
        permission: MemoryPermissionFlags,
    ) -> Result<(), PagingError> {
        if (virtual_address.to_usize() & !PAGE_MASK) != 0 {
            return Err(PagingError::AddressIsNotAligned);
        }
        let entry = self.get_target_entry(pm_manager, virtual_address)?;
        entry.set_permission(permission);
        cpu::flush_data_cache_all();
        Ok(())
    }

    /// Unmap virtual_address.
    ///
    /// This function searches target page table entry and disable present flag.
    /// After disabling, this calls [`Self::cleanup_page_table`] to collect freed page tables.
    /// If target entry does not exist, this function will ignore it and call [`Self::cleanup_page_table`]
    /// when entry_may_be_deleted == true, otherwise this will return [`PagingError::EntryIsNotFound`].
    ///
    /// This does not delete physical address and huge bit from the entry. It disables the present flag only.
    /// It helps [`Self::cleanup_page_table`].
    pub fn unassociate_address(
        &self,
        virtual_address: VAddress,
        pm_manager: &mut PhysicalMemoryManager,
        entry_may_be_deleted: bool,
    ) -> Result<(), PagingError> {
        match self.get_target_leaf_entry(pm_manager, virtual_address, false) {
            Ok(entry) => {
                entry.invalidate();
                cpu::flush_data_cache_all();
                self.cleanup_page_table(virtual_address, pm_manager)
            }
            Err(err) => {
                if err == PagingError::EntryIsNotFound && entry_may_be_deleted {
                    self.cleanup_page_table(virtual_address, pm_manager)
                } else {
                    Err(err)
                }
            }
        }
    }

    /// Unmap virtual_address ~ (virtual_address + size)
    ///
    /// This function searches target page entries and disable present flag.
    /// After disabling, this calls [`Self::cleanup_page_table`] to collect freed page tables.
    /// If target entry does not exist, this function will return Error:EntryIsNotFound.
    /// When a huge table was used and the mapped size is different from expected size, this will return error.
    ///
    /// This does not delete physical address and huge bit from the entry.
    pub fn unassociate_address_width_size(
        &self,
        virtual_address: VAddress,
        mut size: MSize,
        pm_manager: &mut PhysicalMemoryManager,
        entry_may_be_deleted: bool,
    ) -> Result<(), PagingError> {
        if (size & !PAGE_MASK) != 0 {
            return Err(PagingError::AddressIsNotAligned);
        }
        if size == PAGE_SIZE {
            return self.unassociate_address(virtual_address, pm_manager, entry_may_be_deleted);
        }
        let (table_address, level) = self.get_table_and_initial_shit_level(virtual_address)?;
        let virtual_address = Self::get_canonical_address(virtual_address)?;
        let mut v = virtual_address;
        self._associate_area(
            level,
            table_address,
            pm_manager,
            &mut PAddress::new(0),
            &mut v,
            &mut size,
            MemoryPermissionFlags::rodata(),
            MemoryOptionFlags::KERNEL,
            true,
        )?;

        if !size.is_zero() {
            return Err(PagingError::InvalidPageTable);
        }
        cpu::flush_data_cache_all();
        if self._cleanup_page_tables(level, table_address, pm_manager, virtual_address)? {
            Err(PagingError::InvalidPageTable)
        } else {
            Ok(())
        }
    }

    fn _cleanup_page_tables(
        &self,
        level: u8,
        table: &mut [PageTableEntry],
        pm_manager: &mut PhysicalMemoryManager,
        virtual_address: &mut VAddress,
    ) -> Result<bool, PagingError> {
        if level == 0 {
            for e in table {
                if e.is_validated() {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        let index =
            (virtual_address.to_usize() >> (PAGE_SHIFT + 9 * level as usize)) & (table.len() - 1);
        if table[index].has_next() {
            let next_table_address = table[index].get_next_table_address();
            if !self._cleanup_page_tables(
                level - 1,
                unsafe {
                    from_raw_parts_mut(
                        next_table_address.to_usize() as *mut _,
                        NUM_OF_TABLE_ENTRIES,
                    )
                },
                pm_manager,
                virtual_address,
            )? {
                return Ok(false);
            }
            table[index].invalidate();
            /* Free this table */
            pm_manager.free(next_table_address, PAGE_SIZE, false)?;
        }
        if table[index].is_valid() {
            return Ok(false);
        }
        for e in table {
            if e.is_valid() {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Clean up the page table.
    pub fn cleanup_page_table(
        &self,
        virtual_address: VAddress,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), PagingError> {
        let (table, level) = self.get_table_and_initial_level(virtual_address)?;
        if self._cleanup_page_tables(
            level,
            table,
            pm_manager,
            &mut Self::get_canonical_address(virtual_address)?,
        )? {
            Err(PagingError::InvalidPageTable)
        } else {
            cpu::flush_data_cache_all();
            cpu::tlbi_vmalle1is();
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
            .map_err(|e| PagingError::MemoryError(e))
    }

    /// Flush page table and apply new page table.
    ///
    /// This function sets page_table into TTBR.
    /// If Self is for kernel page manager, this function does nothing.
    /// **This function must call after [`Self::init`], otherwise the system may crash.**
    ///
    /// [`init`]: #method.init
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
    /// This function operates `tlbi vaelis`.
    pub fn update_page_cache(virtual_address: VAddress, range: MSize) {
        if range.to_index().to_usize() > 16 {
            Self::update_page_cache_all()
        } else {
            cpu::flush_data_cache_all();
            for i in MIndex::new(0)..range.to_index() {
                cpu::tlbi_vaae1is(((virtual_address & PAGE_MASK) + i.to_offset().to_usize()) as u64)
            }
        }
    }

    /// Delete all TLBs
    ///
    /// This function operates `tlbi vmalle1is`.
    pub fn update_page_cache_all() {
        cpu::flush_data_cache_all();
        cpu::tlbi_vmalle1is();
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
        let print_normal = |v: VAddress, p: PAddress, pm: MemoryPermissionFlags, attr: u64| {
            kprintln!(
                "VA: {:>#16X} => PA: {:>#16X}, W:{:>5}, E:{:>5}, U:{:>5}, AttrIdx:{}",
                v.to_usize(),
                p.to_usize(),
                pm.is_writable(),
                pm.is_executable(),
                pm.is_user_accessible(),
                attr
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
                let attr_index = e.get_memory_attribute_index();
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
                    print_normal(*virtual_address, pa, p, attr_index);
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
                            physical_address_to_direct_map(e.get_next_table_address()),
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
