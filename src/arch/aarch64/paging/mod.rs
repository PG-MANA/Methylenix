//!
//! Paging Manager
//!
//! These modules treat the paging system of AArch64.
//!
//! This does not handle memory status(which process using what memory area).
//! This is the back-end of VirtualMemoryManager.

mod table_entry;

use self::table_entry::{NUM_OF_TABLE_ENTRIES, NUM_OF_TOP_LEVEL_TABLE_ENTRIES, TableEntry};

use crate::arch::target_arch::context::memory_layout::{
    DIRECT_MAP_START_ADDRESS, HIGH_MEMORY_START_ADDRESS, direct_map_to_physical_address,
    physical_address_to_direct_map,
};
use crate::arch::target_arch::device::cpu;

use crate::kernel::memory_manager::{data_type::*, physical_memory_manager::PhysicalMemoryManager};

/// Default Page Size (Type = MSize)
pub const PAGE_SIZE: MSize = MSize::new(PAGE_SIZE_USIZE);

/// Default Page Size (Type = usize)
pub const PAGE_SIZE_USIZE: usize = 0x1000;

/// `PAGE_SIZE = 1 << PAGE_SHIFT` (Type = usize)
pub const PAGE_SHIFT: usize = 12;

/// if !PAGE_MASK & address !=0 => address is not page aligned.
pub const PAGE_MASK: usize = !0xFFF;

/// Default page cache size for paging
pub const PAGING_CACHE_LENGTH: usize = 64;

/// Max virtual address of AArch64 (Type = VAddress)
pub const MAX_VIRTUAL_ADDRESS: VAddress = VAddress::new(MAX_VIRTUAL_ADDRESS_USIZE);

/// Max virtual address of AArch64 (Type = usize)
pub const MAX_VIRTUAL_ADDRESS_USIZE: usize = 0xFFFF_FFFF_FFFF_FFFF;

pub const NEED_COPY_HIGH_MEMORY_PAGE_TABLE: bool = false;

const BLOCK_ENTRY_ENABLED_SHIFT_LEVEL: u8 = (PAGE_SHIFT as u8) + 9 * (3 - 1/* Level 1*/);

static mut MAIR_NORMAL_MEMORY_INDEX: u64 = 0;
const MAIR_NORMAL_MEMORY_ATTRIBUTE: u64 = 0xff;
static mut MAIR_DEVICE_MEMORY_INDEX: u64 = 1;
const MAIR_DEVICE_MEMORY_ATTRIBUTE: u64 = 0;

const SHAREABILITY_NON_SHAREABLE: u64 = 0;
#[allow(dead_code)]
const SHAREABILITY_OUTER_SHAREABLE: u64 = 0b10;
const SHAREABILITY_INNER_SHAREABLE: u64 = 0b11;

#[derive(Clone)]
enum PageTableType {
    Invalid,
    Kernel(VAddress),
    User(VAddress),
}

/// PageManager
///
/// This controls paging system.
/// This manager does not check if specified address is usable,
/// that should be done by VirtualMemoryManager.
#[derive(Clone)]
pub struct PageManager {
    page_table: PageTableType,
    tcr: u64,
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

impl Default for PageManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PageManager {
    /// Create System Page Manager
    /// Before use, **you must call [`Self::init`]**.
    pub const fn new() -> Self {
        Self {
            page_table: PageTableType::Invalid,
            tcr: 0,
        }
    }

    /// Init PageManager
    ///
    /// This function must be called only once on boot time.
    pub fn init(&mut self, pm_manager: &mut PhysicalMemoryManager) -> Result<(), PagingError> {
        let mut mair = cpu::get_mair();
        for i in 0..=8 {
            if i == 8 {
                /* Not Found */
                unsafe { MAIR_NORMAL_MEMORY_INDEX = 0 };
                mair = MAIR_NORMAL_MEMORY_ATTRIBUTE << (unsafe { MAIR_NORMAL_MEMORY_INDEX } << 3);
                break;
            }
            if ((mair >> (i << 3)) & 0xff) == MAIR_NORMAL_MEMORY_ATTRIBUTE {
                unsafe { MAIR_NORMAL_MEMORY_INDEX = i };
                break;
            }
        }
        /* Set new attributes */
        unsafe { MAIR_DEVICE_MEMORY_INDEX = if MAIR_NORMAL_MEMORY_INDEX == 0 { 1 } else { 0 } };
        mair = unsafe {
            (mair & !(0xff << (MAIR_DEVICE_MEMORY_INDEX << 3)))
                | (MAIR_DEVICE_MEMORY_ATTRIBUTE << (MAIR_DEVICE_MEMORY_INDEX << 3))
        };
        unsafe { cpu::set_mair(mair) };
        /* TODO: Setup TCR_EL1 from scratch */
        let tcr_el1 = cpu::get_tcr();
        let t1sz = (tcr_el1 & cpu::TCR_EL1_T1SZ) >> cpu::TCR_EL1_T1SZ_OFFSET;

        /* Set memory address information */
        unsafe {
            HIGH_MEMORY_START_ADDRESS = VAddress::new(((1 << t1sz) - 1) << (64 - t1sz));
            DIRECT_MAP_START_ADDRESS = HIGH_MEMORY_START_ADDRESS;
        }

        /* Set up the page table */
        self.page_table = PageTableType::Kernel(Self::alloc_page_table(pm_manager)?);
        self.tcr = tcr_el1;

        Ok(())
    }

    pub fn init_user(
        &mut self,
        system_page_manager: &Self,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), PagingError> {
        self.page_table = PageTableType::User(Self::alloc_page_table(pm_manager)?);
        self.tcr = system_page_manager.tcr;
        /* TODO: Adjust TCR_EL1 for user */

        Ok(())
    }

    pub fn copy_system_area(&mut self, _: &Self) -> Result<(), PagingError> {
        Ok(())
    }

    const fn get_table_and_initial_shit_level(
        &self,
        virtual_address: VAddress,
    ) -> Result<(VAddress, u8), PagingError> {
        match self.page_table {
            PageTableType::Invalid => Err(PagingError::InvalidPageTable),
            PageTableType::Kernel(a) => {
                if (virtual_address.to_usize() & (1 << (u64::BITS - 1))) != 0 {
                    Ok((
                        a,
                        Self::txsz_to_initial_shift_level(cpu::get_t1sz(self.tcr)),
                    ))
                } else {
                    Err(PagingError::AddressIsNotCanonical)
                }
            }
            PageTableType::User(a) => {
                if (virtual_address.to_usize() & (1 << (u64::BITS - 1))) == 0 {
                    Ok((
                        a,
                        Self::txsz_to_initial_shift_level(cpu::get_t0sz(self.tcr)),
                    ))
                } else {
                    Err(PagingError::AddressIsNotCanonical)
                }
            }
        }
    }

    fn get_canonical_address(address: VAddress) -> Result<VAddress, PagingError> {
        let high_memory_start_address =
            unsafe { *((&raw const HIGH_MEMORY_START_ADDRESS).as_ref().unwrap()) };
        if address.to_usize() & (1usize << (u64::BITS - 1)) != 0 {
            if address >= high_memory_start_address {
                Ok(VAddress::new(
                    address.to_usize() - high_memory_start_address.to_usize(),
                ))
            } else {
                Err(PagingError::AddressIsNotCanonical)
            }
        } else {
            Ok(address)
        }
    }

    fn get_target_descriptor(
        &self,
        virtual_address: VAddress,
        table_address: VAddress,
        shift_level: u8,
    ) -> Result<(&'static mut TableEntry, MSize), PagingError> {
        let index = (virtual_address.to_usize() >> shift_level) & (NUM_OF_TABLE_ENTRIES - 1);
        let table =
            unsafe { &mut *(table_address.to_usize() as *mut [TableEntry; NUM_OF_TABLE_ENTRIES]) };
        if shift_level == PAGE_SHIFT as u8 || table[index].is_block_descriptor() {
            Ok((&mut table[index], MSize::new(1 << shift_level)))
        } else if table[index].is_table_descriptor() {
            self.get_target_descriptor(
                virtual_address,
                physical_address_to_direct_map(table[index].get_next_table_address()),
                shift_level - NUM_OF_TABLE_ENTRIES.trailing_zeros() as u8,
            )
        } else {
            Err(PagingError::EntryIsNotFound)
        }
    }

    fn set_permission_and_options(
        e: &mut TableEntry,
        p: MemoryPermissionFlags,
        o: MemoryOptionFlags,
    ) {
        e.set_permission(p);
        if o.is_device_memory() || o.is_io_map() {
            e.set_memory_attribute_index(unsafe { MAIR_DEVICE_MEMORY_INDEX });
            e.set_shareability(SHAREABILITY_NON_SHAREABLE);
        } else {
            e.set_memory_attribute_index(unsafe { MAIR_NORMAL_MEMORY_INDEX });
            e.set_shareability(SHAREABILITY_INNER_SHAREABLE);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn _associate_address(
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
        let index = (virtual_address.to_usize() >> shift_level) & (NUM_OF_TABLE_ENTRIES - 1);
        let table =
            unsafe { &mut *(table_address.to_usize() as *mut [TableEntry; NUM_OF_TABLE_ENTRIES]) };
        let target_range = MSize::new(1 << shift_level);
        let allow_block =
            option.should_use_huge_page() || option.is_device_memory() || option.is_io_map();
        let mut base = TableEntry::new();
        if !is_unassociation {
            Self::set_permission_and_options(&mut base, permission, option);
        }

        for e in &mut table[index..] {
            if size.is_zero() {
                return Ok(());
            }
            if shift_level == PAGE_SHIFT as _ {
                if is_unassociation {
                    e.invalidate();
                } else {
                    let mut entry = base.clone();
                    entry.set_output_address(*physical_address);
                    entry.validate_as_level3_descriptor();
                    *e = entry;
                }
                *size -= target_range;
                *physical_address += target_range;
                *virtual_address += target_range;
            } else if (allow_block
                && (shift_level <= BLOCK_ENTRY_ENABLED_SHIFT_LEVEL)
                && ((*physical_address & (target_range.to_usize() - 1)) == 0)
                && (*size >= target_range))
                || (is_unassociation && e.is_block_descriptor())
            {
                /* Block Entry */
                if is_unassociation {
                    e.invalidate();
                } else {
                    let mut entry = base.clone();
                    entry.set_output_address(*physical_address);
                    entry.validate_as_block_descriptor();
                    *e = entry;
                }
                *size -= target_range;
                *physical_address += target_range;
                *virtual_address += target_range;
            } else {
                let table_address = if !e.is_table_descriptor() {
                    if e.is_block_descriptor() || is_unassociation {
                        return Err(PagingError::EntryIsNotFound);
                    }
                    Self::alloc_page_table(pm_manager)?
                } else {
                    physical_address_to_direct_map(e.get_next_table_address())
                };
                let r = self._associate_address(
                    shift_level - NUM_OF_TABLE_ENTRIES.trailing_zeros() as u8,
                    table_address,
                    pm_manager,
                    physical_address,
                    virtual_address,
                    size,
                    permission,
                    option,
                    is_unassociation,
                );

                if !e.is_table_descriptor() {
                    if r.is_err() {
                        let _ = pm_manager.free(
                            direct_map_to_physical_address(table_address),
                            PAGE_SIZE,
                            false,
                        );
                    } else {
                        *e = TableEntry::create_table_entry(direct_map_to_physical_address(
                            table_address,
                        ));
                    }
                }
                r?;
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
        let (table_address, initial_shift) =
            self.get_table_and_initial_shit_level(virtual_address)?;

        self._associate_address(
            initial_shift,
            table_address,
            pm_manager,
            &mut physical_address,
            &mut Self::get_canonical_address(virtual_address)?,
            &mut size,
            permission,
            option,
            false,
        )?;
        assert!(size.is_zero());

        cpu::flush_data_cache_all();
        Ok(())
    }

    /// Change permission of virtual_address
    ///
    /// This function searches the target page table entry and changes the permission.
    /// If virtual_address is not valid, this will return [`PagingError::EntryIsNotFound`].
    pub fn change_memory_permission(
        &self,
        _pm_manager: &mut PhysicalMemoryManager,
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
        let (table, shift_level) = self.get_table_and_initial_shit_level(virtual_address)?;
        while s != size {
            let (entry, t) = self.get_target_descriptor(virtual_address, table, shift_level)?;
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
        if (size & !PAGE_MASK) != 0 {
            return Err(PagingError::AddressIsNotAligned);
        }
        let (table_address, initial_shift) =
            self.get_table_and_initial_shit_level(virtual_address)?;
        let virtual_address = Self::get_canonical_address(virtual_address)?;
        let mut v = virtual_address;
        self._associate_address(
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
        assert!(size.is_zero());

        self._cleanup_page_tables(initial_shift, table_address, pm_manager, virtual_address)?;
        cpu::flush_data_cache_all();
        cpu::tlbi_vmalle1is();
        Ok(())
    }

    fn _cleanup_page_tables(
        &self,
        shift_level: u8,
        table_address: VAddress,
        pm_manager: &mut PhysicalMemoryManager,
        virtual_address: VAddress,
    ) -> Result<bool, PagingError> {
        let index = (virtual_address.to_usize() >> shift_level) & (NUM_OF_TABLE_ENTRIES - 1);
        let table = unsafe { &mut *(table_address.to::<[TableEntry; NUM_OF_TABLE_ENTRIES]>()) };

        if shift_level != PAGE_SHIFT as _ && table[index].is_table_descriptor() {
            let next_table_address = table[index].get_next_table_address();
            if !self._cleanup_page_tables(
                shift_level - NUM_OF_TABLE_ENTRIES.trailing_zeros() as u8,
                physical_address_to_direct_map(next_table_address),
                pm_manager,
                virtual_address,
            )? {
                return Ok(false);
            }
            table[index] = TableEntry::new();
            /* Free this table */
            pm_manager
                .free(next_table_address, PAGE_SIZE, false)
                .or(Err(PagingError::MemoryError))?;
        }
        Ok(!table.iter().any(|e| e.is_validated()))
    }

    /// Clean up the page table.
    pub fn cleanup_page_table(
        &self,
        virtual_address: VAddress,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), PagingError> {
        let (table_address, initial_shift) =
            self.get_table_and_initial_shit_level(virtual_address)?;
        if self._cleanup_page_tables(
            initial_shift,
            table_address,
            pm_manager,
            Self::get_canonical_address(virtual_address)?,
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
        if let PageTableType::User(t) = self.page_table {
            fn purge_table(
                table: VAddress,
                shift_level: u8,
                pm_manager: &mut PhysicalMemoryManager,
            ) -> Result<(), PagingError> {
                if shift_level == PAGE_SHIFT as _ {
                    return Ok(());
                }
                let table = unsafe { &mut *(table.to::<[TableEntry; NUM_OF_TABLE_ENTRIES]>()) };
                for e in table {
                    if e.is_table_descriptor() {
                        let next_table_address = e.get_next_table_address();
                        purge_table(
                            physical_address_to_direct_map(next_table_address),
                            shift_level - NUM_OF_TABLE_ENTRIES.trailing_zeros() as u8,
                            pm_manager,
                        )?;
                        *e = TableEntry::new();
                        /* Free this table */
                        pm_manager
                            .free(next_table_address, PAGE_SIZE, false)
                            .or(Err(PagingError::MemoryError))?;
                    }
                }
                Ok(())
            }

            let (table_address, initial_shift) =
                self.get_table_and_initial_shit_level(VAddress::new(0))?;
            assert_eq!(table_address, t);
            purge_table(table_address, initial_shift, pm_manager)?;

            pm_manager
                .free(direct_map_to_physical_address(t), PAGE_SIZE, false)
                .or(Err(PagingError::MemoryCacheOverflowed))?;
            self.page_table = PageTableType::Invalid;
            Ok(())
        } else {
            Err(PagingError::InvalidPageTable)
        }
    }

    /// Allocate the page table.
    fn alloc_page_table(pm_manager: &mut PhysicalMemoryManager) -> Result<VAddress, PagingError> {
        match pm_manager.alloc(PAGE_SIZE, MOrder::new(PAGE_SHIFT)) {
            Ok(p) => {
                let table_address = physical_address_to_direct_map(p);
                let table =
                    unsafe { &mut *(table_address.to::<[TableEntry; NUM_OF_TABLE_ENTRIES]>()) };
                for e in table {
                    *e = TableEntry::new();
                }
                cpu::flush_data_cache_all();
                Ok(table_address)
            }
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
    /// This function sets page_table into TTBR.
    /// If Self is for kernel page manager, this function does nothing.
    /// **This function must call after [`Self::init`], otherwise the system may crash.**
    pub fn flush_page_table(&mut self) {
        cpu::flush_data_cache_all();
        match self.page_table {
            PageTableType::Invalid => { /* Do nothing */ }
            PageTableType::User(a) => {
                let mut tcr = cpu::get_tcr();
                let mask = cpu::TCR_EL1_TBI0
                    | cpu::TCR_EL1_TG0
                    | cpu::TCR_EL1_SH0
                    | cpu::TCR_EL1_ORGN0
                    | cpu::TCR_EL1_IRGN0
                    | cpu::TCR_EL1_EPD0
                    | cpu::TCR_EL1_T0SZ;
                tcr &= !mask;
                tcr |= self.tcr & mask;
                unsafe { cpu::set_tcr(tcr) };
                unsafe { cpu::set_ttbr0(direct_map_to_physical_address(a).to_usize() as u64) };
                cpu::tlbi_vmalle1is();
            }
            PageTableType::Kernel(a) => {
                let mut tcr = cpu::get_tcr();
                /* Set except settings for TTBR0_EL1 */
                let mask = !(cpu::TCR_EL1_TBI0
                    | cpu::TCR_EL1_TG0
                    | cpu::TCR_EL1_SH0
                    | cpu::TCR_EL1_ORGN0
                    | cpu::TCR_EL1_IRGN0
                    | cpu::TCR_EL1_EPD0
                    | cpu::TCR_EL1_T0SZ);
                tcr &= !mask;
                tcr |= self.tcr & mask;
                unsafe { cpu::set_tcr(tcr) };
                unsafe { cpu::set_ttbr1(direct_map_to_physical_address(a).to_usize() as u64) };
                cpu::tlbi_vmalle1is();
            }
        }
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

    #[allow(clippy::too_many_arguments)]
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
        let table =
            unsafe { &*(table_address.to::<[TableEntry; NUM_OF_TOP_LEVEL_TABLE_ENTRIES]>()) };
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

        let ((table_address, initial_shift), base) = match self.page_table {
            PageTableType::Invalid => {
                kprintln!("Invalid Page Table");
                return;
            }
            PageTableType::User(_) => {
                if let Some(s) = start
                    && s >= unsafe { HIGH_MEMORY_START_ADDRESS }
                {
                    kprintln!("Invalid start_address: {}", s);
                    return;
                }
                (
                    self.get_table_and_initial_shit_level(VAddress::new(0))
                        .unwrap(),
                    0,
                )
            }
            PageTableType::Kernel(_) => {
                if let Some(e) = end
                    && e < unsafe { HIGH_MEMORY_START_ADDRESS }
                {
                    kprintln!("Invalid end_address: {}", e);
                    return;
                }
                (
                    self.get_table_and_initial_shit_level(unsafe { HIGH_MEMORY_START_ADDRESS })
                        .unwrap(),
                    unsafe { HIGH_MEMORY_START_ADDRESS }.to_usize(),
                )
            }
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
