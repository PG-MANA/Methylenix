//!
//! Paging Manager
//!
//! These modules treat the paging system of x86_64.
//! Currently, this module can handle 4K, 2M, and 1G paging.
//!
//! This does not handle memory status(which process using what memory area).
//! This is the back-end of VirtualMemoryManager.
//!
//! The paging system of x86_64 is the system translate from "linear-address" to physical address.
//! With 4-level paging, virtual address is substantially same as linear address.
//! VirtualMemoryManager call this manager to set up the translation from virtual address to physical address.
//! Therefore, the name of argument is unified as not "linear address" but "virtual address".

mod pde;
mod pdpte;
mod pml4e;
mod pte;

use self::{
    pde::{PD_MAX_ENTRY, PDE},
    pdpte::{PDPT_MAX_ENTRY, PDPTE},
    pml4e::{PML4_MAX_ENTRY, PML4E},
    pte::{PT_MAX_ENTRY, PTE},
};

use crate::arch::target_arch::context::memory_layout::{
    CANONICAL_AREA_HIGH, direct_map_to_physical_address, physical_address_to_direct_map,
};
use crate::arch::target_arch::device::cpu;

use crate::kernel::memory_manager::{data_type::*, physical_memory_manager::PhysicalMemoryManager};

/// Default Page Size(Type = MSize)
pub const PAGE_SIZE: MSize = MSize::new(PAGE_SIZE_USIZE);

/// Default Page Size(Type = usize)
pub const PAGE_SIZE_USIZE: usize = 0x1000;

/// `PAGE_SIZE = 1 << PAGE_SHIFT` (Type = usize)
pub const PAGE_SHIFT: usize = 12;

/// if !PAGE_MASK & address !=0 => address is not page aligned.
pub const PAGE_MASK: usize = !0xFFF;

/// Default page cache size for paging
pub const PAGING_CACHE_LENGTH: usize = 64;

/// Max virtual address of x86_64(Type = VAddress)
pub const MAX_VIRTUAL_ADDRESS: VAddress = VAddress::new(MAX_VIRTUAL_ADDRESS_USIZE);

/// Max virtual address of x86_64(Type = usize)
pub const MAX_VIRTUAL_ADDRESS_USIZE: usize = 0xFFFF_FFFF_FFFF_FFFF;

pub const NEED_COPY_HIGH_MEMORY_PAGE_TABLE: bool = true;

/// PageManager
///
/// This controls paging system.
/// This manager does not check if specified address is usable,
/// that should be done by VirtualMemoryManager.
#[derive(Clone)]
pub struct PageManager {
    pml4: VAddress,
    is_1gb_paging_supported: bool,
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

/// PagingEntry
///
/// This trait is to treat PML4, PDPTE, PDE, and PTE as the common way.
#[allow(dead_code)]
trait PagingEntry {
    fn is_present(&self) -> bool;
    fn set_present(&mut self, b: bool);
    fn is_writable(&self) -> bool;
    fn set_writable(&mut self, b: bool);
    #[allow(dead_code)]
    fn is_user_accessible(&self) -> bool;
    fn set_user_accessible(&mut self, b: bool);
    fn set_wtc(&mut self, b: bool);
    fn set_disable_cache(&mut self, b: bool);
    fn is_accessed(&self) -> bool;
    fn off_accessed(&mut self);
    fn is_dirty(&self) -> bool;
    fn off_dirty(&mut self);
    fn set_global(&mut self, b: bool);
    fn is_no_execute(&self) -> bool;
    fn set_no_execute(&mut self, b: bool);
    fn is_huge(&self) -> bool;
    fn get_address(&self) -> Option<PAddress>;
    fn set_address(&mut self, address: PAddress) -> bool;
    fn get_map_size(&self) -> MSize;
}

impl PageManager {
    /// Create InterruptManager with invalid data.
    ///
    /// Before use, **you must call [`Self::init`]**.
    pub const fn new() -> PageManager {
        PageManager {
            pml4: VAddress::new(0),
            is_1gb_paging_supported: false,
        }
    }

    /// Init PageManager
    ///
    /// This function will take one page from cache_memory_list and set up it to PML4.
    /// After that, this will associate address of PML4(in the process, some pages are associated).
    pub fn init(&mut self, pm_manager: &mut PhysicalMemoryManager) -> Result<(), PagingError> {
        self.is_1gb_paging_supported = {
            let mut eax: u32 = 0x80000001;
            let mut ebx: u32 = 0;
            let mut ecx: u32 = 0;
            let mut edx: u32 = 0;
            unsafe {
                cpu::cpuid(&mut eax, &mut ebx, &mut ecx, &mut edx);
            }
            (edx & (1 << 26)) != 0
        };
        let pml4_address = Self::alloc_page_table(pm_manager)?;
        self.pml4 = pml4_address;
        let pml4_table = self.get_top_level_table();
        for pml4 in pml4_table.iter_mut() {
            pml4.init();
        }
        Ok(())
    }

    pub fn init_user(
        &mut self,
        system_page_manager: &Self,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), PagingError> {
        self.is_1gb_paging_supported = false;
        let pml4_address = Self::alloc_page_table(pm_manager)?;
        self.pml4 = pml4_address;
        for pml4e in self.get_top_level_table().iter_mut() {
            pml4e.init();
        }
        self.copy_system_area(system_page_manager)?;
        Ok(())
    }

    pub fn copy_system_area(&mut self, system_page_manager: &Self) -> Result<(), PagingError> {
        let pml4_table = self.get_top_level_table();
        let high_area_start =
            (CANONICAL_AREA_HIGH.start().to_usize() >> (PAGE_SHIFT + 9 * 3)) & (0x1FF);
        let system_pml4_table = system_page_manager.get_top_level_table();
        for i in high_area_start..pml4_table.len() {
            pml4_table[i] = system_pml4_table[i].clone();
        }
        Ok(())
    }

    const fn get_top_level_table(&self) -> &mut [PML4E; PML4_MAX_ENTRY] {
        unsafe { &mut *(self.pml4.to::<[PML4E; PML4_MAX_ENTRY]>()) }
    }

    /// Search the target PDPTE linked with virtual_address.
    ///
    /// This function calculates the index number of the PDPTE linked with the virtual_address.
    /// When PML4 does not exist, this will make PML4 if `should_create_entry` is true,
    /// otherwise return [`PagingError::EntryIsNotFound`].
    fn get_target_pdpte(
        &self,
        pm_manager: &mut PhysicalMemoryManager,
        virtual_address: VAddress,
        should_create_entry: bool,
    ) -> Result<&'static mut PDPTE, PagingError> {
        if (virtual_address.to_usize() & !PAGE_MASK) != 0 {
            return Err(PagingError::AddressIsNotAligned);
        }

        let pml4_table = self.get_top_level_table();

        let number_of_pml4e = (virtual_address.to_usize() >> (PAGE_SHIFT + 9 * 3)) & (0x1FF);
        let number_of_pdpte = (virtual_address.to_usize() >> (PAGE_SHIFT + 9 * 2)) & (0x1FF);

        let pml4e = &mut pml4_table[number_of_pml4e];
        if !pml4e.is_present() {
            if !should_create_entry {
                return Err(PagingError::EntryIsNotFound);
            }
            let pdpt_address = Self::alloc_page_table(pm_manager)?;
            let pdpt = unsafe { &mut *(pdpt_address.to::<[PDPTE; PDPT_MAX_ENTRY]>()) };
            for entry in pdpt.iter_mut() {
                entry.init();
            }
            pml4e.init();
            pml4e.set_address(direct_map_to_physical_address(pdpt_address));
            pml4e.set_present(true);
        }
        if pml4e.is_huge() {
            pr_err!("PML4E does not have the next page table");
            return Err(PagingError::InvalidPageTable);
        }

        let pdpte = &mut unsafe {
            &mut *(physical_address_to_direct_map(pml4e.get_address().unwrap())
                .to::<[PDPTE; PDPT_MAX_ENTRY]>())
        }[number_of_pdpte];
        Ok(pdpte)
    }

    /// Search the target PDE linked with virtual_address.
    ///
    /// This function calculates the index number of the PDE linked with the virtual_address.
    /// If pdpte is none, this will call [`Self::get_target_pdpte`].
    /// If PML4 or PDPTE does not exist, this will make them if `should_create_entry` is true,
    /// otherwise return [`PagingError::EntryIsNotFound`].
    fn get_target_pde(
        &self,
        pm_manager: &mut PhysicalMemoryManager,
        virtual_address: VAddress,
        should_create_entry: bool,
        pdpte: Option<&mut PDPTE>,
    ) -> Result<&'static mut PDE, PagingError> {
        let number_of_pde = (virtual_address.to_usize() >> (PAGE_SHIFT + 9)) & (0x1FF);

        let pdpte = if let Some(p) = pdpte {
            p
        } else {
            self.get_target_pdpte(pm_manager, virtual_address, should_create_entry)?
        };

        if !pdpte.is_present() {
            if !should_create_entry {
                return Err(PagingError::EntryIsNotFound);
            }
            let pd_address = Self::alloc_page_table(pm_manager)?;
            let pd = unsafe { &mut *(pd_address.to::<[PDE; PD_MAX_ENTRY]>()) };
            for entry in pd.iter_mut() {
                entry.init();
            }
            pdpte.init();
            pdpte.set_address(direct_map_to_physical_address(pd_address));
            pdpte.set_present(true);
        }
        if pdpte.is_huge() {
            pr_err!("PDPTE does not have the next page table");
            return Err(PagingError::InvalidPageTable);
        }
        let pde = &mut unsafe {
            &mut *(physical_address_to_direct_map(pdpte.get_address().unwrap())
                .to::<[PDE; PD_MAX_ENTRY]>())
        }[number_of_pde];
        Ok(pde)
    }

    /// Search the target PTE linked with virtual_address.
    ///
    /// This function calculates the index number of the PTE linked with the virtual_address.
    /// If pde is none, this will call [`Self::get_target_pde`].
    /// If PML4, PDPTE, or PDE does not exist, this will make them if `should_create_entry` is true,
    /// otherwise return [`PagingError::EntryIsNotFound`].
    fn get_target_pte(
        &self,
        pm_manager: &mut PhysicalMemoryManager,
        virtual_address: VAddress,
        should_create_entry: bool,
        pde: Option<&mut PDE>,
    ) -> Result<&'static mut PTE, PagingError> {
        let number_of_pte = (virtual_address.to_usize() >> PAGE_SHIFT) & (0x1FF);

        let pde = if let Some(p) = pde {
            p
        } else {
            self.get_target_pde(pm_manager, virtual_address, should_create_entry, None)?
        };
        if !pde.is_present() {
            if !should_create_entry {
                return Err(PagingError::EntryIsNotFound);
            }
            let pt_address = Self::alloc_page_table(pm_manager)?;
            let pt = unsafe { &mut *(pt_address.to::<[PTE; PT_MAX_ENTRY]>()) };
            for entry in pt.iter_mut() {
                entry.init();
            }
            pde.init();
            pde.set_address(direct_map_to_physical_address(pt_address));
            pde.set_present(true);
        }
        if pde.is_huge() {
            pr_err!("PDE does not have the next page table");
            return Err(PagingError::InvalidPageTable);
        }
        let pte = &mut unsafe {
            &mut *(physical_address_to_direct_map(pde.get_address().unwrap())
                .to::<[PTE; PT_MAX_ENTRY]>())
        }[number_of_pte];
        Ok(pte)
    }

    /// Search the target ;aging entry linked with virtual_address.
    ///
    /// This function calculates the index number of the PDPTE, PDE, and PTE linked with the virtual_address.
    /// If huge bit is present(means 1GB or 2MB paging is enabled), return with terminal page entry.
    /// If PML4 or PDPTE or PDE does not exist, this will make them if `should_create_entry` is true,
    /// otherwise return [`PagingError::EntryIsNotFound`].
    fn get_target_page_entry(
        &self,
        pm_manager: &mut PhysicalMemoryManager,
        virtual_address: VAddress,
        should_create_entry: bool,
    ) -> Result<&'static mut dyn PagingEntry, PagingError> {
        let pdpte = self.get_target_pdpte(pm_manager, virtual_address, should_create_entry)?;
        if pdpte.is_present() && pdpte.is_huge() {
            return Ok(pdpte);
        }
        let pde = self.get_target_pde(
            pm_manager,
            virtual_address,
            should_create_entry,
            Some(pdpte),
        )?;
        if pdpte.is_present() && pde.is_huge() {
            return Ok(pde);
        }
        let pte =
            self.get_target_pte(pm_manager, virtual_address, should_create_entry, Some(pde))?;
        Ok(pte)
    }

    /// Map virtual_address to physical address
    ///
    /// This function will map from virtual_address to virtual_address + size.
    /// This function is used to map consecutive physical address.
    /// This may use 2MB or 1GB paging when [`MemoryOptionFlags::should_use_huge_page`] or
    /// [`MemoryOptionFlags::is_device_memory`] or [`MemoryOptionFlags::is_io_map`] is true.
    pub fn associate_address(
        &self,
        pm_manager: &mut PhysicalMemoryManager,
        physical_address: PAddress,
        virtual_address: VAddress,
        size: MSize,
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
        let allow_huge =
            option.should_use_huge_page() || option.is_device_memory() || option.is_io_map();
        let mut processed_size = MSize::new(0);

        while processed_size <= size {
            let virtual_address = virtual_address + processed_size;
            let physical_address = physical_address + processed_size;
            let number_of_pde = (virtual_address.to_usize() >> (PAGE_SHIFT + 9)) & (0x1FF);
            let number_of_pte = (virtual_address.to_usize() >> PAGE_SHIFT) & (0x1FF);

            let pdpte = self.get_target_pdpte(pm_manager, virtual_address, true)?;

            if allow_huge
                && self.is_1gb_paging_supported
                && number_of_pde == 0
                && number_of_pte == 0
                && (physical_address & 0x3FFFFFFF) == 0
                && (size - processed_size) >= MSize::new(0x40000000)
                && (!pdpte.is_present() || pdpte.is_huge())
            {
                /* Use 1GB paging */
                pdpte.init();
                pdpte.set_huge(true);
                pdpte.set_no_execute(!permission.is_executable());
                pdpte.set_writable(permission.is_writable());
                pdpte.set_user_accessible(permission.is_user_accessible());
                pdpte.set_address(physical_address);
                pdpte.set_present(true);
                processed_size += MSize::new(0x40000000);
                continue;
            }

            let pde = self.get_target_pde(pm_manager, virtual_address, true, Some(pdpte))?;

            if allow_huge
                && number_of_pte == 0
                && (physical_address & 0x1FFFFF) == 0
                && (size - processed_size) >= MSize::new(0x200000)
                && (!pde.is_present() || pde.is_huge())
            {
                /* Use 2MB paging */
                pde.init();
                pde.set_huge(true);
                pde.set_no_execute(!permission.is_executable());
                pde.set_writable(permission.is_writable());
                pde.set_user_accessible(permission.is_user_accessible());
                pde.set_address(physical_address);
                pde.set_present(true);
                processed_size += MSize::new(0x200000);
                continue;
            }

            /* 4KiB */
            let pte = self.get_target_pte(pm_manager, virtual_address, true, Some(pde))?;
            pte.init();
            pte.set_address(physical_address);
            pte.set_no_execute(!permission.is_executable());
            pte.set_writable(permission.is_writable());
            pte.set_user_accessible(permission.is_user_accessible());
            pte.set_present(true);
            processed_size += PAGE_SIZE;
        }
        Ok(())
    }

    /// Change permission of virtual_address
    ///
    /// This function searches the target page entry(usually PTE) and change permission.
    /// If virtual_address is not valid, this will return [`PagingError::EntryIsNotFound`].
    pub fn change_memory_permission(
        &self,
        pm_manager: &mut PhysicalMemoryManager,
        virtual_address: VAddress,
        size: MSize,
        permission: MemoryPermissionFlags,
        _: MemoryOptionFlags,
    ) -> Result<(), PagingError> {
        if (virtual_address.to_usize() & !PAGE_MASK) != 0 {
            return Err(PagingError::AddressIsNotAligned);
        } else if (size.to_usize() & !PAGE_MASK) != 0 {
            return Err(PagingError::SizeIsNotAligned);
        }
        let mut s = MSize::new(0);
        while s != size {
            let entry = self.get_target_page_entry(pm_manager, virtual_address, false)?;
            let t = entry.get_map_size();
            if s + t > size {
                return Err(PagingError::InvalidPageTable);
            }
            entry.set_writable(permission.is_writable());
            entry.set_no_execute(!permission.is_executable());
            entry.set_user_accessible(permission.is_user_accessible());
            s += t;
        }
        Ok(())
    }

    /// Unmap virtual_address ~ (virtual_address + size)
    ///
    /// This function searches target page entry(PDPTE, PDE, PTE) and disable present flag.
    /// After disabling, this calls [`Self::cleanup_page_table`] to collect freed page tables.
    /// If target entry is not exists, this function will return [`PagingError:EntryIsNotFound`].
    /// When huge table was used and the mapped size is different from expected size, this will return error.
    pub fn unassociate_address(
        &self,
        virtual_address: VAddress,
        size: MSize,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), PagingError> {
        if (size & !PAGE_MASK) != 0 {
            return Err(PagingError::AddressIsNotAligned);
        }

        let mut processed_size = MSize::new(0);
        while processed_size < size {
            let processing_virtual_address = virtual_address + processed_size;
            let pdpte = self.get_target_pdpte(pm_manager, processing_virtual_address, false)?;
            if !pdpte.is_present() {
                return Err(PagingError::EntryIsNotFound);
            }
            if pdpte.is_huge() {
                let map_size = pdpte.get_map_size();
                if (size - processed_size) < map_size {
                    return Err(PagingError::InvalidPageTable);
                }
                pdpte.set_present(false);
                processed_size += map_size;
                self.cleanup_page_table(processing_virtual_address, pm_manager)?;
                continue;
            }
            let pde =
                self.get_target_pde(pm_manager, processing_virtual_address, false, Some(pdpte))?;
            if !pde.is_present() {
                return Err(PagingError::EntryIsNotFound);
            }
            if pde.is_huge() {
                let map_size = pde.get_map_size();
                if (size - processed_size) < map_size {
                    return Err(PagingError::InvalidPageTable);
                }
                pde.set_present(false);
                processed_size += map_size;
                self.cleanup_page_table(processing_virtual_address, pm_manager)?;
                continue;
            }
            let pte =
                self.get_target_pte(pm_manager, processing_virtual_address, false, Some(pde))?;
            if !pte.is_present() {
                return Err(PagingError::EntryIsNotFound);
            }
            pte.set_present(false);
            self.cleanup_page_table(processing_virtual_address, pm_manager)?;
            processed_size += PAGE_SIZE;
        }
        Ok(())
    }

    /// Clean up the page table.
    ///
    /// This function searches non-used page tables(PDPT, PD, PT) and puts them into cache_memory_list.
    /// Currently, this function always returns OK(()).  
    pub fn cleanup_page_table(
        &self,
        virtual_address: VAddress,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), PagingError> {
        let number_of_pml4e = (virtual_address.to_usize() >> (PAGE_SHIFT + 9 * 3)) & (0x1FF);
        let number_of_pdpte = (virtual_address.to_usize() >> (PAGE_SHIFT + 9 * 2)) & (0x1FF);
        let number_of_pde = (virtual_address.to_usize() >> (PAGE_SHIFT + 9)) & (0x1FF);
        let pml4e = &mut self.get_top_level_table()[number_of_pml4e];
        if !pml4e.is_present() {
            return Ok(());
        }

        let pdpte = &mut unsafe {
            &mut *(physical_address_to_direct_map(pml4e.get_address().unwrap())
                .to::<[PDPTE; PDPT_MAX_ENTRY]>())
        }[number_of_pdpte];
        if pdpte.is_present() && !pdpte.is_huge() {
            let pde = &mut unsafe {
                &mut *(physical_address_to_direct_map(pdpte.get_address().unwrap())
                    .to::<[PDE; PD_MAX_ENTRY]>())
            }[number_of_pde];
            if pde.is_present() {
                /* Try to free PT */
                if !pde.is_huge() {
                    if unsafe {
                        &*(physical_address_to_direct_map(pde.get_address().unwrap())
                            .to::<[PTE; PT_MAX_ENTRY]>())
                    }
                    .iter()
                    .find(|e| e.is_present())
                    .is_some()
                    {
                        return Ok(());
                    }
                    /* Free PT */
                    pm_manager
                        .free(pde.get_address().unwrap(), PAGE_SIZE, false)
                        .or(Err(PagingError::MemoryError))?;
                    pde.set_present(false);
                }
            }
            /* Try to free PD */
            if unsafe {
                &*(physical_address_to_direct_map(pdpte.get_address().unwrap())
                    .to::<[PDE; PD_MAX_ENTRY]>())
            }
            .iter()
            .find(|e| e.is_present())
            .is_some()
            {
                return Ok(());
            }
            /* Free PD */
            pm_manager
                .free(pdpte.get_address().unwrap(), PAGE_SIZE, false)
                .or(Err(PagingError::MemoryError))?;
            pdpte.set_present(false);
        }
        /* Try to free PDPT */
        if unsafe {
            &*(physical_address_to_direct_map(pml4e.get_address().unwrap())
                .to::<[PDPTE; PDPT_MAX_ENTRY]>())
        }
        .iter()
        .find(|e| e.is_present())
        .is_some()
        {
            return Ok(());
        }
        /* Free PDPT */
        pm_manager
            .free(pml4e.get_address().unwrap(), PAGE_SIZE, false)
            .or(Err(PagingError::MemoryError))?;
        pml4e.set_present(false);

        Ok(())
    }

    pub fn destroy_page_table(
        &mut self,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), PagingError> {
        /* TODO: clean up all user entries and purge page tables */
        pm_manager
            .free(direct_map_to_physical_address(self.pml4), PAGE_SIZE, false)
            .or(Err(PagingError::MemoryCacheOverflowed))?;
        self.pml4 = VAddress::new(0);
        Ok(())
    }

    /// Allocate the page table.
    fn alloc_page_table(pm_manager: &mut PhysicalMemoryManager) -> Result<VAddress, PagingError> {
        match pm_manager.alloc(PAGE_SIZE, MOrder::new(PAGE_SHIFT)) {
            Ok(p) => Ok(physical_address_to_direct_map(p)),
            Err(_) => Err(PagingError::MemoryCacheRanOut),
        }
    }

    /// Flush page table and apply new page table.
    ///
    /// This function sets PML4 into CR3.
    /// **This function must call after [`Self::init`], otherwise system may crash.**
    pub fn flush_page_table(&mut self) {
        unsafe {
            cpu::set_cr3(direct_map_to_physical_address(self.pml4).to_usize());
        }
    }

    /// Flush page table and apply new page table.
    ///
    /// This function will return PML4 address.
    /// **This function must call after [`Self::init`], otherwise system may crash.**
    pub fn get_page_table_address(&self) -> PAddress {
        direct_map_to_physical_address(self.pml4)
    }

    /// Delete the paging cache of the target address and update it.
    ///
    /// This function operates invlpg.
    pub fn update_page_cache(virtual_address: VAddress, range: MSize) {
        for i in MIndex::new(0)..range.to_index() {
            unsafe { cpu::invlpg((virtual_address + i.to_offset()).to_usize()) };
        }
    }

    /// Delete all TLBs
    ///
    /// This function operates nothing
    pub fn update_page_cache_all() {}

    /// Dump paging table
    ///
    /// This function shows the status of paging, it prints a lot.
    pub fn dump_table(&self, start: Option<VAddress>, end: Option<VAddress>) {
        let mut permission = (false /* writable */, false /* no_execute */);
        let mut omitted = false;
        let mut last_address = (
            VAddress::new(0), /* virtual address */
            PAddress::new(0), /* physical address */
        );
        let print_normal = |v: usize, p: usize, w: bool, e: bool, a: bool, s: &str| {
            kprintln!(
                "Linear addresses: {:>#16X} => Physical Address: {:>#16X}, W:{:>5}, E:{:>5}, A:{:>5} {}",
                v,
                p,
                w,
                e,
                a,
                s
            );
        };
        let print_omitted = |v: usize, p: usize| {
            kprintln!(
                "...               {:>#16X}                      {:>#16X} (fin)",
                v,
                p
            );
        };
        let calculate_virtual_address = |pml4_count: usize,
                                         pdpte_count: usize,
                                         pde_count: usize,
                                         pte_count: usize|
         -> VAddress {
            let address = (pml4_count << (PAGE_SHIFT + 9 * 3))
                | (pdpte_count << (PAGE_SHIFT + 9 * 2))
                | (pde_count << (PAGE_SHIFT + 9))
                | (pte_count << PAGE_SHIFT);
            VAddress::new(if (address & (1 << 47)) != 0 {
                (0xffff << 48) | address
            } else {
                address
            })
        };

        let pml4_table = self.get_top_level_table();
        for (pml4_count, pml4) in pml4_table.iter().enumerate() {
            if !pml4.is_present() {
                continue;
            }
            let pdpt = unsafe {
                &*(physical_address_to_direct_map(pml4.get_address().unwrap())
                    .to::<[PDPTE; PDPT_MAX_ENTRY]>())
            };
            for (pdpte_count, pdpte) in pdpt.iter().enumerate() {
                if !pdpte.is_present() {
                    continue;
                }
                if pdpte.is_huge() {
                    let virtual_address = calculate_virtual_address(pml4_count, pdpte_count, 0, 0);
                    if start.is_some() && virtual_address < start.unwrap() {
                        continue;
                    }
                    if last_address.0 + MSize::new(1 << (PAGE_SHIFT + 9 * 2)) == virtual_address
                        && last_address.1 + MSize::new(1 << (PAGE_SHIFT + 9 * 2))
                            == pdpte.get_address().unwrap()
                        && permission.0 == pdpte.is_writable()
                        && permission.1 == pdpte.is_no_execute()
                    {
                        last_address.0 = virtual_address;
                        last_address.1 = pdpte.get_address().unwrap();
                        omitted = true;
                        continue;
                    }
                    if omitted {
                        print_omitted(last_address.0.to_usize(), last_address.1.to_usize());
                        omitted = false;
                    }
                    print_normal(
                        virtual_address.to_usize(),
                        pdpte.get_address().unwrap().to_usize(),
                        pdpte.is_writable(),
                        !pdpte.is_no_execute(),
                        pdpte.is_accessed(),
                        "1G",
                    );
                    if end.is_some() && virtual_address >= end.unwrap() {
                        return;
                    }
                    last_address.0 = virtual_address;
                    last_address.1 = pdpte.get_address().unwrap();
                    permission = (pdpte.is_writable(), pdpte.is_no_execute());
                    continue;
                }
                let pd = unsafe {
                    &*(physical_address_to_direct_map(pdpte.get_address().unwrap())
                        .to::<[PDE; PD_MAX_ENTRY]>())
                };
                for (pde_count, pde) in pd.iter().enumerate() {
                    if !pde.is_present() {
                        continue;
                    }
                    if pde.is_huge() {
                        let virtual_address =
                            calculate_virtual_address(pml4_count, pdpte_count, pde_count, 0);
                        if start.is_some() && virtual_address < start.unwrap() {
                            continue;
                        }
                        if last_address.0 + MSize::new(1 << (PAGE_SHIFT + 9)) == virtual_address
                            && last_address.1 + MSize::new(1 << (PAGE_SHIFT + 9))
                                == pde.get_address().unwrap()
                            && permission.0 == pde.is_writable()
                            && permission.1 == pde.is_no_execute()
                        {
                            last_address.0 = virtual_address;
                            last_address.1 = pde.get_address().unwrap();
                            omitted = true;
                            continue;
                        }
                        if omitted {
                            print_omitted(last_address.0.to_usize(), last_address.1.to_usize());
                            omitted = false;
                        }
                        print_normal(
                            virtual_address.to_usize(),
                            pde.get_address().unwrap().to_usize(),
                            pdpte.is_writable(),
                            !pdpte.is_no_execute(),
                            pdpte.is_accessed(),
                            "2M",
                        );
                        if end.is_some() && virtual_address >= end.unwrap() {
                            return;
                        }
                        last_address.0 = virtual_address;
                        last_address.1 = pde.get_address().unwrap();
                        permission = (pde.is_writable(), pde.is_no_execute());
                        continue;
                    }
                    let pt = unsafe {
                        &*(physical_address_to_direct_map(pde.get_address().unwrap())
                            .to::<[PTE; PT_MAX_ENTRY]>())
                    };
                    for (pte_count, pte) in pt.iter().enumerate() {
                        if !pte.is_present() {
                            continue;
                        }
                        let virtual_address = calculate_virtual_address(
                            pml4_count,
                            pdpte_count,
                            pde_count,
                            pte_count,
                        );
                        if start.is_some() && virtual_address < start.unwrap() {
                            continue;
                        }
                        if last_address.0 + MSize::new(1 << PAGE_SHIFT) == virtual_address
                            && last_address.1 + MSize::new(1 << PAGE_SHIFT)
                                == pte.get_address().unwrap()
                            && permission.0 == pte.is_writable()
                            && permission.1 == pte.is_no_execute()
                        {
                            last_address.0 = virtual_address;
                            last_address.1 = pte.get_address().unwrap();
                            omitted = true;
                            continue;
                        }
                        if omitted {
                            print_omitted(last_address.0.to_usize(), last_address.1.to_usize());
                            omitted = false;
                        }
                        print_normal(
                            virtual_address.to_usize(),
                            pte.get_address().unwrap().to_usize(),
                            pte.is_writable(),
                            !pte.is_no_execute(),
                            pte.is_accessed(),
                            "4K",
                        );
                        if end.is_some() && virtual_address >= end.unwrap() {
                            return;
                        }
                        last_address.0 = virtual_address;
                        last_address.1 = pte.get_address().unwrap();
                        permission = (pte.is_writable(), pte.is_no_execute());
                    }
                }
            }
        }
    }
}
