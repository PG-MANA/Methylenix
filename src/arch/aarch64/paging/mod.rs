//!
//! Paging Manager
//!
//! This modules treat the paging system of AArch64.
//!
//! This does not handle memory status(which process using what memory area).
//! This is the back-end of VirtualMemoryManager.

mod table_entry;

use self::table_entry::{TableEntry, NUM_OF_TABLE_ENTRIES, NUM_OF_TOP_LEVEL_TABLE_ENTRIES};

use crate::arch::target_arch::context::memory_layout::{
    direct_map_to_physical_address, physical_address_to_direct_map,
};
use crate::arch::target_arch::device::cpu;

use crate::kernel::memory_manager::data_type::{
    Address, MOrder, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress, VAddress,
};
use crate::kernel::memory_manager::physical_memory_manager::PhysicalMemoryManager;

/// Default Page Size, the mainly using 4KiB paging.(Type = MSize)
pub const PAGE_SIZE: MSize = MSize::from(PAGE_SIZE_USIZE);

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

pub const NEED_COPY_HIGH_MEMORY_PAGE_TABLE: bool = false;

const TTBR1_TABLE_ADDRESS_MASK: u64 = ((1 << 48) - 1) ^ 1;
#[allow(dead_code)]
const TTBR0_TABLE_ADDRESS_MASK: u64 = ((1 << 48) - 1) ^ 1;

const BLOCK_ENTRY_ENABLED_SHIFT_LEVEL: u8 = (PAGE_SHIFT as u8) + 9 * (3 - 1/* Level 1*/);

const MAIR_NORMAL_MEMORY_INDEX: u64 = 0;
const MAIR_NORMAL_MEMORY_ATTRIBUTE: u64 = 0xff;
const MAIR_DEVICE_MEMORY_INDEX: u64 = 1;
const MAIR_DEVICE_MEMORY_ATTRIBUTE: u64 = 0;

const SHAREABILITY_NON_SHAREABLE: u64 = 0;
#[allow(dead_code)]
const SHAREABILITY_OUTER_SHAREABLE: u64 = 0b10;
const SHAREABILITY_INNER_SHAREABLE: u64 = 0b11;

/// PageManager
///
/// This controls paging system.
/// This manager does not check if specified address is usable,
/// that should done by VirtualMemoryManager.
#[derive(Clone)]
pub struct PageManager {
    page_table: Option<VAddress>,
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
}

impl PageManager {
    /// Create System Page Manager
    /// Before use, **you must call [`init`]**.
    ///
    /// [`init`]: #method.init
    pub const fn new() -> Self {
        Self { page_table: None }
    }

    /// Init PageManager
    ///
    /// Currently, this function does nothing.
    pub fn init(&mut self, _: &mut PhysicalMemoryManager) -> Result<(), PagingError> {
        let mair = (MAIR_DEVICE_MEMORY_ATTRIBUTE << (MAIR_DEVICE_MEMORY_INDEX << 3))
            | (MAIR_NORMAL_MEMORY_ATTRIBUTE << (MAIR_NORMAL_MEMORY_INDEX << 3));
        unsafe { cpu::set_mair(mair) };
        return Ok(());
    }

    pub fn init_user(
        &mut self,
        _: &Self,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), PagingError> {
        self.page_table = Some(Self::alloc_page_table(pm_manager)?);
        for e in self.get_user_table().unwrap().iter_mut() {
            *e = TableEntry::new();
        }
        return Ok(());
    }

    pub fn copy_system_area(&mut self, _: &Self) -> Result<(), PagingError> {
        return Ok(());
    }

    const fn get_user_table(&self) -> Option<&mut [TableEntry; NUM_OF_TOP_LEVEL_TABLE_ENTRIES]> {
        if let Some(e) = self.page_table {
            Some(unsafe {
                &mut *(e.to_usize() as *mut [TableEntry; NUM_OF_TOP_LEVEL_TABLE_ENTRIES])
            })
        } else {
            None
        }
    }

    fn get_table_and_initial_shit_level(
        &self,
        virtual_address: VAddress,
    ) -> Result<(VAddress, u8), PagingError> {
        if (virtual_address.to_usize() >> (u64::BITS - 16)) == 0xffff {
            unsafe {
                Ok((
                    physical_address_to_direct_map(PAddress::new(
                        (cpu::get_ttbr1() & TTBR1_TABLE_ADDRESS_MASK) as usize,
                    )),
                    Self::txsz_to_initial_shift_level(cpu::get_t1sz()),
                ))
            }
        } else if let Some(t) = self.page_table {
            Ok((t, unsafe {
                Self::txsz_to_initial_shift_level(cpu::get_t0sz())
            }))
        } else {
            Err(PagingError::EntryIsNotFound)
        }
    }

    fn _get_target_level3_descriptor(
        &self,
        pm_manager: &mut PhysicalMemoryManager,
        virtual_address: VAddress,
        table_address: VAddress,
        shift_level: u8,
        should_create_entry: bool,
    ) -> Result<&'static mut TableEntry, PagingError> {
        let index = (virtual_address.to_usize() >> shift_level) & (NUM_OF_TABLE_ENTRIES - 1);
        let table =
            unsafe { &mut *(table_address.to_usize() as *mut [TableEntry; NUM_OF_TABLE_ENTRIES]) };
        if shift_level == PAGE_SHIFT as u8 {
            return Ok(&mut table[index]);
        }
        if !table[index].is_table_descriptor() {
            if table[index].is_block_descriptor() || !should_create_entry {
                return Err(PagingError::EntryIsNotFound);
            }
            let new_table_address = Self::alloc_page_table(pm_manager)?;
            let new_table = unsafe {
                &mut *(new_table_address.to_usize() as *mut [TableEntry; NUM_OF_TABLE_ENTRIES])
            };
            for e in new_table {
                *e = TableEntry::new();
            }
            let result = self._get_target_level3_descriptor(
                pm_manager,
                virtual_address,
                new_table_address,
                shift_level - NUM_OF_TABLE_ENTRIES.trailing_zeros() as u8,
                should_create_entry,
            );
            table[index] =
                TableEntry::create_table_entry(direct_map_to_physical_address(new_table_address));
            result
        } else {
            self._get_target_level3_descriptor(
                pm_manager,
                virtual_address,
                physical_address_to_direct_map(table[index].get_next_table_address()),
                shift_level - NUM_OF_TABLE_ENTRIES.trailing_zeros() as u8,
                should_create_entry,
            )
        }
    }

    fn get_target_level3_descriptor(
        &self,
        pm_manager: &mut PhysicalMemoryManager,
        virtual_address: VAddress,
        should_create_entry: bool,
    ) -> Result<&'static mut TableEntry, PagingError> {
        let (table_address, initial_shift) =
            self.get_table_and_initial_shit_level(virtual_address)?;
        self._get_target_level3_descriptor(
            pm_manager,
            virtual_address,
            table_address,
            initial_shift,
            should_create_entry,
        )
    }

    fn _get_target_descriptor(
        &self,
        pm_manager: &mut PhysicalMemoryManager,
        virtual_address: VAddress,
        table_address: VAddress,
        shift_level: u8,
    ) -> Result<&'static mut TableEntry, PagingError> {
        if shift_level == PAGE_SHIFT as u8 {
            let index = (virtual_address.to_usize() >> shift_level) & (NUM_OF_TABLE_ENTRIES - 1);
            let table_address = unsafe {
                &mut *(table_address.to_usize() as *mut [TableEntry; NUM_OF_TABLE_ENTRIES])
            };
            return Ok(&mut table_address[index]);
        }
        let index = (virtual_address.to_usize() >> shift_level) & (NUM_OF_TABLE_ENTRIES - 1);
        let table =
            unsafe { &mut *(table_address.to_usize() as *mut [TableEntry; NUM_OF_TABLE_ENTRIES]) };
        if table[index].is_block_descriptor() {
            Ok(&mut table[index])
        } else if table[index].is_table_descriptor() {
            self._get_target_descriptor(
                pm_manager,
                virtual_address,
                physical_address_to_direct_map(table[index].get_next_table_address()),
                shift_level - NUM_OF_TABLE_ENTRIES.trailing_zeros() as u8,
            )
        } else {
            Err(PagingError::EntryIsNotFound)
        }
    }

    fn get_target_descriptor_descriptor(
        &self,
        pm_manager: &mut PhysicalMemoryManager,
        virtual_address: VAddress,
    ) -> Result<&'static mut TableEntry, PagingError> {
        let (table_address, initial_shift) =
            self.get_table_and_initial_shit_level(virtual_address)?;
        self._get_target_descriptor(pm_manager, virtual_address, table_address, initial_shift)
    }

    fn set_permission_and_options(
        e: &mut TableEntry,
        p: MemoryPermissionFlags,
        o: MemoryOptionFlags,
    ) {
        e.set_permission(p);
        if o.is_device_memory() || o.is_io_map() {
            e.set_memory_attribute_index(MAIR_DEVICE_MEMORY_INDEX);
            e.set_shareability(SHAREABILITY_NON_SHAREABLE); /* OK..? */
        } else {
            e.set_memory_attribute_index(MAIR_NORMAL_MEMORY_INDEX);
            e.set_shareability(SHAREABILITY_INNER_SHAREABLE);
        }
    }

    /// Associate physical address with virtual_address.
    ///
    /// This function will get target PTE from virtual_address
    /// (if not exist, will make)and set physical address.
    /// "permission" will be used when set the PTE attribute.
    /// If you want to associate wide area(except physical address is non-linear),
    /// you should use [`associate_area`].(it may use 2MB paging).
    ///
    /// This function does not flush page table and invoke page cache. You should do them manually.
    ///
    /// [`associate_area`]: #method.associate_area
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

        let entry = self.get_target_level3_descriptor(pm_manager, virtual_address, true)?;
        entry.init();
        entry.set_output_address(physical_address);
        Self::set_permission_and_options(entry, permission, option);
        entry.validate_as_level3_descriptor();
        Ok(())
    }

    fn _associate_area(
        &self,
        shift_level: u8,
        table_address: VAddress,
        pm_manager: &mut PhysicalMemoryManager,
        physical_address: &mut PAddress,
        virtual_address: &mut VAddress,
        size: &mut MSize,
        permission: MemoryPermissionFlags,
        option: MemoryOptionFlags,
        is_unassociation: bool,
    ) -> Result<(), PagingError> {
        if shift_level == PAGE_SHIFT as u8 {
            let mut index =
                (virtual_address.to_usize() >> shift_level) & (NUM_OF_TABLE_ENTRIES - 1);
            let table = unsafe {
                &mut *(table_address.to_usize() as *mut [TableEntry; NUM_OF_TABLE_ENTRIES])
            };
            let mut entry = TableEntry::new();
            Self::set_permission_and_options(&mut entry, permission, option);
            if is_unassociation {
                entry.invalidate();
            } else {
                entry.validate_as_level3_descriptor();
            }
            let entry = entry;
            while !(*size).is_zero() && index < NUM_OF_TABLE_ENTRIES {
                let mut e = entry.clone();
                e.set_output_address(*physical_address);
                table[index] = e;

                let mapped_size = MSize::new(1 << shift_level);
                *size -= mapped_size;
                *physical_address += mapped_size;
                *virtual_address += mapped_size;
                index += 1;
            }
            return Ok(());
        }
        let mut index = (virtual_address.to_usize() >> shift_level) & (NUM_OF_TABLE_ENTRIES - 1);
        let table =
            unsafe { &mut *(table_address.to_usize() as *mut [TableEntry; NUM_OF_TABLE_ENTRIES]) };
        while !(*size).is_zero() && index < NUM_OF_TABLE_ENTRIES {
            if (shift_level <= BLOCK_ENTRY_ENABLED_SHIFT_LEVEL)
                && ((*physical_address & ((1 << shift_level) - 1)) == 0)
                && (*size >= MSize::new(1 << shift_level))
            {
                /* Block Entry */
                let mut entry = TableEntry::new();
                Self::set_permission_and_options(&mut entry, permission, option);
                if is_unassociation {
                    entry.invalidate();
                } else {
                    entry.validate_as_block_descriptor();
                }
                let entry = entry;
                while !(*size).is_zero()
                    && index < NUM_OF_TABLE_ENTRIES
                    && *size >= MSize::new(1 << shift_level)
                {
                    let mut e = entry.clone();
                    e.set_output_address(*physical_address);
                    table[index] = e;

                    let mapped_size = MSize::new(1 << shift_level);
                    *size -= mapped_size;
                    *physical_address += mapped_size;
                    *virtual_address += mapped_size;
                    index += 1;
                }
                if (*size).is_zero() || index == NUM_OF_TABLE_ENTRIES {
                    return Ok(());
                }
            }
            if !table[index].is_table_descriptor() {
                if table[index].is_block_descriptor() || is_unassociation {
                    return Err(PagingError::EntryIsNotFound);
                }
                let new_table_address = Self::alloc_page_table(pm_manager)?;
                let new_table = unsafe {
                    &mut *(new_table_address.to_usize() as *mut [TableEntry; NUM_OF_TABLE_ENTRIES])
                };
                for e in new_table {
                    *e = TableEntry::new();
                }
                if let Err(e) = self._associate_area(
                    shift_level - NUM_OF_TABLE_ENTRIES.trailing_zeros() as u8,
                    new_table_address,
                    pm_manager,
                    physical_address,
                    virtual_address,
                    size,
                    permission,
                    option,
                    is_unassociation,
                ) {
                    let _ = pm_manager.free(
                        direct_map_to_physical_address(new_table_address),
                        PAGE_SIZE,
                        false,
                    );
                    return Err(e);
                }
                table[index] = TableEntry::create_table_entry(direct_map_to_physical_address(
                    new_table_address,
                ));
            } else {
                self._associate_area(
                    shift_level - NUM_OF_TABLE_ENTRIES.trailing_zeros() as u8,
                    physical_address_to_direct_map(table[index].get_next_table_address()),
                    pm_manager,
                    physical_address,
                    virtual_address,
                    size,
                    permission,
                    option,
                    is_unassociation,
                )?;
            }
            index += 1;
        }
        return Ok(());
    }

    /// Map virtual_address to physical address with size.
    ///
    /// This function will map from virtual_address to virtual_address + size.
    /// This function is used to map consecutive physical address.
    /// This may use 2MB or 1GB paging.
    /// If you want to map non-consecutive physical address,
    /// you should call [`associate_address`] repeatedly.
    ///
    /// This function does not flush page table and invoke page cache. You should do them manually.
    ///
    /// [`associate_address`]: #method.associate_address
    pub fn associate_area(
        &self,
        pm_manager: &mut PhysicalMemoryManager,
        mut physical_address: PAddress,
        mut virtual_address: VAddress,
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
        if size == PAGE_SIZE {
            return self.associate_address(
                pm_manager,
                physical_address,
                virtual_address,
                permission,
                option,
            );
        }
        let (table_address, initial_shift) =
            self.get_table_and_initial_shit_level(virtual_address)?;

        self._associate_area(
            initial_shift,
            table_address,
            pm_manager,
            &mut physical_address,
            &mut virtual_address,
            &mut size,
            permission,
            option,
            false,
        )?;
        if !size.is_zero() {
            pr_err!("Size is not zero: {}", size);
            Err(PagingError::EntryIsNotFound)
        } else {
            Ok(())
        }
    }

    /// Change permission of virtual_address
    ///
    /// This function searches the target page entry(usually PTE) and change permission.
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
        let entry = self.get_target_descriptor_descriptor(pm_manager, virtual_address)?;
        entry.set_permission(permission);
        Ok(())
    }

    /// Unmap virtual_address.
    ///
    /// This function searches target page entry(usually PTE) and disable present flag.
    /// After disabling, this calls [`Self::cleanup_page_table`] to collect freed page tables.
    /// If target entry is not exists, this function will ignore it and call [`Self::cleanup_page_table`]
    /// when entry_may_be_deleted == true, otherwise this will return PagingError:PagingError::EntryIsNotFound.
    ///
    /// This does not delete physical address and huge bit from the entry. it  disable present flag only.
    /// It helps [`Self::cleanup_page_table`].
    pub fn unassociate_address(
        &self,
        virtual_address: VAddress,
        pm_manager: &mut PhysicalMemoryManager,
        entry_may_be_deleted: bool,
    ) -> Result<(), PagingError> {
        match self.get_target_level3_descriptor(pm_manager, virtual_address, false) {
            Ok(entry) => {
                entry.invalidate();
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
    /// This function searches target page entry(PDPTE, PDE, PTE) and disable present flag.
    /// After disabling, this calls [`Self::cleanup_page_table`] to collect freed page tables.
    /// If target entry is not exists, this function will return Error:EntryIsNotFound.
    /// When huge table was used and the mapped size is different from expected size, this will return error.
    ///
    /// This does not delete physical address and huge bit from the entry. it  disable present flag only.
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
        let (table_address, initial_shift) =
            self.get_table_and_initial_shit_level(virtual_address)?;

        let mut v = virtual_address;
        self._associate_area(
            initial_shift,
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
        if self._cleanup_page_tables(initial_shift, table_address, pm_manager, virtual_address)? {
            Err(PagingError::InvalidPageTable)
        } else {
            Ok(())
        }
    }

    fn _cleanup_page_tables(
        &self,
        shift_level: u8,
        table_address: VAddress,
        pm_manager: &mut PhysicalMemoryManager,
        virtual_address: VAddress,
    ) -> Result<bool, PagingError> {
        if shift_level == PAGE_SHIFT as u8 {
            let table = unsafe {
                &*(table_address.to_usize() as *const [TableEntry; NUM_OF_TABLE_ENTRIES])
            };
            for e in table {
                if e.is_validated() {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        let index = (virtual_address.to_usize() >> shift_level) & (NUM_OF_TABLE_ENTRIES - 1);
        let table =
            unsafe { &mut *(table_address.to_usize() as *mut [TableEntry; NUM_OF_TABLE_ENTRIES]) };
        for index in index..NUM_OF_TABLE_ENTRIES {
            if table[index].is_table_descriptor() {
                let next_table_address = table[index].get_next_table_address();
                if !self._cleanup_page_tables(
                    shift_level - NUM_OF_TABLE_ENTRIES.trailing_zeros() as u8,
                    physical_address_to_direct_map(next_table_address),
                    pm_manager,
                    virtual_address,
                )? {
                    return Ok(false);
                }
                table[index].invalidate();
                /* Free this table */
                if let Err(_e) = pm_manager.free(next_table_address, PAGE_SIZE, false) {
                    return Err(PagingError::MemoryCacheOverflowed);
                }
            }
            if table[index].is_validated() {
                return Ok(false);
            }
        }
        return Ok(true);
    }

    /// Clean up the page table.
    pub fn cleanup_page_table(
        &self,
        virtual_address: VAddress,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), PagingError> {
        let (table_address, initial_shift) =
            self.get_table_and_initial_shit_level(virtual_address)?;
        if self._cleanup_page_tables(initial_shift, table_address, pm_manager, virtual_address)? {
            Err(PagingError::InvalidPageTable)
        } else {
            Ok(())
        }
    }

    /// Allocate the page table.
    fn alloc_page_table(pm_manager: &mut PhysicalMemoryManager) -> Result<VAddress, PagingError> {
        match pm_manager.alloc(PAGE_SIZE, MOrder::new(PAGE_SHIFT)) {
            Ok(p) => Ok(physical_address_to_direct_map(p)),
            Err(_) => Err(PagingError::MemoryCacheRanOut),
        }
    }

    const fn txsz_to_initial_shift_level(txsz: u64) -> u8 {
        (PAGE_SHIFT as u8)
            + 9 * (3
                - (4 - (1
                    + (((u64::BITS as u8) - (txsz as u8) - (PAGE_SHIFT as u8) - 1)
                        / ((PAGE_SHIFT as u8) - 3))) as i8) as u8)
    }

    /// Flush page table and apply new page table.
    ///
    /// This function sets PML4 into CR3.
    /// **This function must call after [`init`], otherwise system may crash.**
    ///
    /// [`init`]: #method.init
    pub fn flush_page_table(&mut self) {
        if let Some(t) = self.page_table {
            unsafe { cpu::set_ttbr0(direct_map_to_physical_address(t).to_usize() as u64) };
        }
    }

    /// Delete the paging cache of the target address and update it.
    ///
    /// This function operates invpg.
    pub fn update_page_cache(_virtual_address: VAddress) {
        /* TODO: inspect why hangs up when exec tlbi on QEMU */
        //unsafe { cpu::tlbi_va((virtual_address & PAGE_MASK) as u64) }
    }

    fn _dump_table(
        &self,
        start_address: VAddress,
        end_address: VAddress,
        table_address: VAddress,
        virtual_address: &mut VAddress,
        shift: u8,
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
        let size = MSize::new(1 << shift);
        let table = unsafe {
            &*(table_address.to_usize() as *const [TableEntry; NUM_OF_TOP_LEVEL_TABLE_ENTRIES])
        };
        for e in table {
            if *virtual_address >= end_address {
                return;
            } else if *virtual_address < start_address {
                *virtual_address += size;
                continue;
            }
            if !e.is_validated() {
                *virtual_address += size;
                if *omitted {
                    print_omitted(continued_address.0, continued_address.1);
                    *omitted = false;
                }
                continue;
            }
            if (shift == PAGE_SHIFT as u8 && e.is_level3_descriptor()) || e.is_block_descriptor() {
                if shift != PAGE_SHIFT as u8 && !e.is_block_descriptor() {
                    kprintln!("Invalid Entry: VA: {:#16X}", virtual_address.to_usize());
                    return;
                } else {
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
                            continued_address.0 += size;
                            continued_address.1 += size;
                            *virtual_address += size;
                            continue;
                        }
                    } else if *virtual_address == continued_address.0
                        && pa == continued_address.1
                        && p == continued_address.2
                    {
                        continued_address.0 += size;
                        continued_address.1 += size;
                        *virtual_address += size;
                        *omitted = true;
                        continue;
                    } else {
                        print_normal(*virtual_address, pa, p, attr_index);
                        *virtual_address += size;
                        continued_address.0 = *virtual_address;
                        continued_address.1 = pa + size;
                        continued_address.2 = p;
                    }
                }
            } else if e.is_table_descriptor() {
                self._dump_table(
                    start_address,
                    end_address,
                    physical_address_to_direct_map(e.get_next_table_address()),
                    virtual_address,
                    shift - NUM_OF_TABLE_ENTRIES.trailing_zeros() as u8,
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

        let ((table_address, initial_shift), base) = if self.page_table.is_some() {
            if let Some(s) = start {
                if s >= VAddress::new(0xffff_0000_0000_0000) {
                    kprintln!("Invalid start_address: {}", s);
                    return;
                }
            }
            (
                self.get_table_and_initial_shit_level(VAddress::new(0))
                    .unwrap(),
                0,
            )
        } else {
            if let Some(e) = end {
                if e < VAddress::new(0xffff_0000_0000_0000) {
                    kprintln!("Invalid end_address: {}", e);
                    return;
                }
            }
            (
                self.get_table_and_initial_shit_level(VAddress::new(0xffff_0000_0000_0000))
                    .unwrap(),
                0xffff_0000_0000_0000usize,
            )
        };
        let mut omitted = false;
        let mut continued_address = (
            VAddress::new(0),
            PAddress::new(0),
            MemoryPermissionFlags::new(false, false, false, false),
        );

        self._dump_table(
            start.unwrap_or(VAddress::new(0)),
            end.unwrap_or(VAddress::new(usize::MAX)),
            table_address,
            &mut VAddress::new(base),
            initial_shift,
            &mut continued_address,
            &mut omitted,
        );
        if omitted {
            print_omitted(continued_address.0, continued_address.1);
        }
    }
}
