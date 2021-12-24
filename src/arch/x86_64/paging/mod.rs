//!
//! Paging Manager
//!
//! This modules treat the paging system of x86_64.
//! Currently this module can handle 4K, 2M, and 1G paging.
//!
//! This does not handle memory status(which process using what memory area).
//! This is the back-end of VirtualMemoryManager.
//!
//! The paging system of x86_64 is the system translate from "linear-address" to physical address.
//! With 4-level paging, virtual address is substantially same as linear address.
//! VirtualMemoryManager call this manager to setup the translation from virtual address to physical address.
//! Therefore, the name of argument is unified as not "linear address" but "virtual address".

mod pde;
mod pdpte;
mod pml4e;
mod pte;

use self::pde::{PDE, PD_MAX_ENTRY};
use self::pdpte::{PDPTE, PDPT_MAX_ENTRY};
use self::pml4e::{PML4E, PML4_MAX_ENTRY};
use self::pte::{PTE, PT_MAX_ENTRY};

use crate::arch::target_arch::context::memory_layout::{
    direct_map_to_physical_address, physical_address_to_direct_map,
};
use crate::arch::target_arch::device::cpu;

//use crate::kernel::memory_manager::physical_memory_manager::PhysicalMemoryManager;
use crate::kernel::memory_manager::data_type::{
    Address, MOrder, MSize, MemoryPermissionFlags, PAddress, VAddress,
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

/// Max virtual address of x86_64(Type = VAddress)
pub const MAX_VIRTUAL_ADDRESS: VAddress = VAddress::new(MAX_VIRTUAL_ADDRESS_USIZE);

/// Max virtual address of x86_64(Type = usize)
pub const MAX_VIRTUAL_ADDRESS_USIZE: usize = 0xFFFF_FFFF_FFFF_FFFF;

/// PageManager
///
/// This controls paging system.
/// This manager does not check if specified address is usable,
/// that should done by VirtualMemoryManager.
#[derive(Clone)]
pub struct PageManager {
    pml4: VAddress,
    is_1gb_paging_supported: bool,
}

/// Paging Error enum
///
/// This enum is used to pass error from PageManager.
#[derive(Eq, PartialEq, Debug)]
pub enum PagingError {
    MemoryCacheRanOut,
    MemoryCacheOverflowed,
    EntryIsNotFound,
    AddressIsNotAligned,
    AddressIsNotCanonical,
    SizeIsNotAligned,
}

/// PagingEntry
///
/// This trait is to treat PML4, PDPTE, PDE, and PTE as the common way.
trait PagingEntry {
    fn is_present(&self) -> bool;
    fn set_present(&mut self, b: bool);
    fn is_writable(&self) -> bool;
    fn set_writable(&mut self, b: bool);
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
    fn get_address(&self) -> Option<PAddress>;
    fn set_address(&mut self, address: PAddress) -> bool;
}

impl PageManager {
    /// Create InterruptManager with invalid data.
    ///
    /// Before use, **you must call [`init`]**.
    ///
    /// [`init`]: #method.init
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
        return Ok(());
    }

    const fn get_top_level_table(&self) -> &mut [PML4E; PML4_MAX_ENTRY] {
        unsafe { &mut *(self.pml4.to_usize() as *mut [PML4E; PML4_MAX_ENTRY]) }
    }

    /// Search the target PDPTE linked with virtual_address.
    ///
    /// This function calculates the index number of the PDPTE linked with the virtual_address.
    /// If PML4 does not exist, this will make PML4 (if should_create == true)
    /// or return PagingError::EntryIsNotFound.
    fn get_target_pdpte(
        &self,
        pm_manager: &mut PhysicalMemoryManager,
        virtual_address: VAddress,
        should_set_present: bool,
        should_set_parent_entry_present: bool,
        should_create_entry: bool,
    ) -> Result<&'static mut PDPTE, PagingError> {
        if (virtual_address.to_usize() & !PAGE_MASK) != 0 {
            return Err(PagingError::AddressIsNotAligned);
        }

        let pml4_table = self.get_top_level_table();

        let number_of_pml4e = (virtual_address.to_usize() >> (PAGE_SHIFT + 9 * 3)) & (0x1FF);
        let number_of_pdpte = (virtual_address.to_usize() >> (PAGE_SHIFT + 9 * 2)) & (0x1FF);

        let pml4e = &mut pml4_table[number_of_pml4e];
        if !pml4e.is_address_set() {
            if !should_create_entry {
                return Err(PagingError::EntryIsNotFound);
            }
            let pdpt_address = Self::alloc_page_table(pm_manager)?;
            let temp_pdpt =
                unsafe { &mut *(pdpt_address.to_usize() as *mut [PDPTE; PDPT_MAX_ENTRY]) };
            for entry in temp_pdpt.iter_mut() {
                entry.init();
            }
            pml4e.init();
            pml4e.set_address(direct_map_to_physical_address(pdpt_address));
        }
        if should_set_parent_entry_present {
            pml4e.set_present(true);
        }

        let pdpte = &mut unsafe {
            &mut *(physical_address_to_direct_map(pml4e.get_address().unwrap()).to_usize()
                as *mut [PDPTE; PDPT_MAX_ENTRY])
        }[number_of_pdpte];
        if should_set_present {
            pdpte.set_present(true);
        }
        Ok(pdpte)
    }

    /// Search the target PDE linked with virtual_address.
    ///
    /// This function calculates the index number of the PDE linked with the virtual_address.
    /// If pdpte is none, this will call [`get_target_pdpte`].
    /// If PML4 or PDPTE does not exist, this will make them (if should_create == true)
    /// or return PagingError::EntryIsNotFound.
    ///
    /// [`get_target_pdpte`]: #method.get_target_pdpte
    fn get_target_pde(
        &self,
        pm_manager: &mut PhysicalMemoryManager,
        virtual_address: VAddress,
        should_set_present: bool,
        should_set_parent_entry_present: bool,
        should_create_entry: bool,
        pdpte: Option<&'static mut PDPTE>,
    ) -> Result<&'static mut PDE, PagingError> {
        let number_of_pde = (virtual_address.to_usize() >> (PAGE_SHIFT + 9 * 1)) & (0x1FF);

        let pdpte = if let Some(p) = pdpte {
            p
        } else {
            self.get_target_pdpte(
                pm_manager,
                virtual_address,
                should_set_parent_entry_present,
                should_set_parent_entry_present,
                should_create_entry,
            )?
        };

        if !pdpte.is_address_set() {
            if !should_create_entry {
                return Err(PagingError::EntryIsNotFound);
            }
            let pd_address = Self::alloc_page_table(pm_manager)?;
            let temp_pd = unsafe { &mut *(pd_address.to_usize() as *mut [PDE; PD_MAX_ENTRY]) };
            for entry in temp_pd.iter_mut() {
                entry.init();
            }
            pdpte.init();
            pdpte.set_address(direct_map_to_physical_address(pd_address));
            if should_set_parent_entry_present {
                pdpte.set_present(true);
            }
        }
        let pde = &mut unsafe {
            &mut *(physical_address_to_direct_map(pdpte.get_address().unwrap()).to_usize()
                as *mut [PDE; PD_MAX_ENTRY])
        }[number_of_pde];
        if should_set_present {
            pde.set_present(true);
        }
        Ok(pde)
    }

    /// Search the target PTE linked with virtual_address.
    ///
    /// This function calculates the index number of the PTE linked with the virtual_address.
    /// If pde is none, this will call [`get_target_pde`].
    /// If PML4, PDPTE, or PDE does not exist, this will make them (if should_create == true)
    /// or return PagingError::EntryIsNotFound.
    ///
    /// [`get_target_pde`]: #method.get_target_pde
    fn get_target_pte(
        &self,
        pm_manager: &mut PhysicalMemoryManager,
        virtual_address: VAddress,
        should_set_parent_entry_present: bool,
        should_create_entry: bool,
        pde: Option<&'static mut PDE>,
    ) -> Result<&'static mut PTE, PagingError> {
        let number_of_pte = (virtual_address.to_usize() >> (PAGE_SHIFT + 9 * 0)) & (0x1FF);

        let pde = if let Some(p) = pde {
            p
        } else {
            self.get_target_pde(
                pm_manager,
                virtual_address,
                should_set_parent_entry_present,
                should_set_parent_entry_present,
                should_create_entry,
                None,
            )?
        };
        if !pde.is_address_set() {
            if !should_create_entry {
                return Err(PagingError::EntryIsNotFound);
            }
            let pt_address = Self::alloc_page_table(pm_manager)?;
            let temp_pt = unsafe { &mut *(pt_address.to_usize() as *mut [PTE; PT_MAX_ENTRY]) };
            for entry in temp_pt.iter_mut() {
                entry.init();
            }
            pde.init();
            pde.set_address(direct_map_to_physical_address(pt_address));
            if should_set_parent_entry_present {
                pde.set_present(true);
            }
        }
        Ok(&mut unsafe {
            &mut *(physical_address_to_direct_map(pde.get_address().unwrap()).to_usize()
                as *mut [PTE; PT_MAX_ENTRY])
        }[number_of_pte])
    }

    /// Search the target ;aging entry linked with virtual_address.
    ///
    /// This function calculates the index number of the PDPTE, PDE, and PTE linked with the virtual_address.
    /// If huge bit is present(means 1GB or 2MB paging is enabled), return with terminal page entry.
    /// If PML4 or PDPTE or PDE does not exist, this will make them (if should_create == true)
    /// or return PagingError::EntryIsNotFound.
    fn get_target_paging_entry(
        &self,
        pm_manager: &mut PhysicalMemoryManager,
        virtual_address: VAddress,
        should_set_present: bool,
        should_set_parent_entry_present: bool,
        should_create_entry: bool,
    ) -> Result<&'static mut dyn PagingEntry, PagingError> {
        let pdpte = self.get_target_pdpte(
            pm_manager,
            virtual_address,
            should_set_present,
            should_set_parent_entry_present,
            should_create_entry,
        )?;
        if pdpte.is_huge() {
            if should_set_present {
                pdpte.set_present(true);
            }
            return Ok(pdpte);
        }
        if should_set_parent_entry_present {
            pdpte.set_present(true);
        }
        let pde = self.get_target_pde(
            pm_manager,
            virtual_address,
            should_set_present,
            should_set_parent_entry_present,
            should_create_entry,
            Some(pdpte),
        )?;
        if pde.is_huge() {
            if should_set_present {
                pde.set_present(true);
            }
            return Ok(pde);
        }
        if should_set_parent_entry_present {
            pde.set_present(true);
        }
        let pte = self.get_target_pte(
            pm_manager,
            virtual_address,
            should_set_parent_entry_present,
            should_create_entry,
            Some(pde),
        )?;
        if should_set_present {
            pte.set_present(true);
        }
        Ok(pte)
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
    ) -> Result<(), PagingError> {
        if ((physical_address.to_usize() & !PAGE_MASK) != 0)
            || ((virtual_address.to_usize() & !PAGE_MASK) != 0)
        {
            return Err(PagingError::AddressIsNotAligned);
        }
        if (virtual_address.to_usize() >> 48) != 0
            && ((virtual_address.to_usize() >> 48) != 0xffff
                || ((virtual_address.to_usize() >> 47) & 1) == 0)
        {
            return Err(PagingError::AddressIsNotCanonical);
        }

        let pte = self.get_target_pte(pm_manager, virtual_address, true, true, None)?;
        pte.init();
        pte.set_address(physical_address);
        pte.set_no_execute(!permission.is_executable());
        pte.set_writable(permission.is_writable());
        pte.set_user_accessible(permission.is_user_accessible());
        pte.set_present(true);
        /* PageManager::reset_paging_local(virtual_address) */
        Ok(())
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
        physical_address: PAddress,
        virtual_address: VAddress,
        size: MSize,
        permission: MemoryPermissionFlags,
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
            );
        }

        let mut processed_size = MSize::new(0);
        while processed_size <= size {
            let processing_virtual_address = virtual_address + processed_size;
            let processing_physical_address = physical_address + processed_size;
            let number_of_pde =
                (processing_virtual_address.to_usize() >> (PAGE_SHIFT + 9 * 1)) & (0x1FF);
            let number_of_pte =
                (processing_virtual_address.to_usize() >> (PAGE_SHIFT + 9 * 0)) & (0x1FF);

            if number_of_pde == 0
                && number_of_pte == 0
                && (processing_physical_address & 0x3FFFFFFF) == 0
                && (size - processed_size) >= MSize::new(0x40000000)
                && self.is_1gb_paging_supported
            {
                /* try to apply 1GB paging */
                let pdpte = self.get_target_pdpte(
                    pm_manager,
                    processing_virtual_address,
                    false,
                    true,
                    true,
                )?;
                /* check if PDPTE is used */
                if !pdpte.is_present() {
                    /* PDPTE is free, we can use 1GB paging! */
                    pdpte.init();
                    pdpte.set_huge(true);
                    pdpte.set_no_execute(!permission.is_executable());
                    pdpte.set_writable(permission.is_writable());
                    pdpte.set_user_accessible(permission.is_user_accessible());
                    pdpte.set_address(processing_physical_address);
                    pdpte.set_present(true);
                    processed_size += MSize::new(0x40000000);
                    continue;
                }
            }
            /* try to apply 2MiB paging */
            if number_of_pte == 0
                && (processing_physical_address & 0x1FFFFF) == 0
                && (size - processed_size) >= MSize::new(0x200000)
            {
                let pde = self.get_target_pde(
                    pm_manager,
                    processing_virtual_address,
                    false,
                    true,
                    true,
                    None,
                )?;
                if !pde.is_present() {
                    pde.init();
                    pde.set_huge(true);
                    pde.set_no_execute(!permission.is_executable());
                    pde.set_writable(permission.is_writable());
                    pde.set_user_accessible(permission.is_user_accessible());
                    pde.set_address(processing_physical_address);
                    pde.set_present(true);
                    processed_size += MSize::new(0x200000);
                    continue;
                }
            }
            /* 4KiB */
            self.associate_address(
                pm_manager,
                processing_physical_address,
                processing_virtual_address,
                permission,
            )?;
            processed_size += PAGE_SIZE;
        }
        Ok(())
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
        let entry =
            self.get_target_paging_entry(pm_manager, virtual_address, false, false, false)?;
        entry.set_writable(permission.is_writable());
        entry.set_no_execute(!permission.is_executable());
        entry.set_user_accessible(permission.is_user_accessible());
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
        match self.get_target_paging_entry(pm_manager, virtual_address, false, false, false) {
            Ok(entry) => {
                entry.set_present(false);
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
        size: MSize,
        pm_manager: &mut PhysicalMemoryManager,
        entry_may_be_deleted: bool,
    ) -> Result<(), PagingError> {
        if (size & !PAGE_MASK) != 0 {
            return Err(PagingError::AddressIsNotAligned);
        }
        if size == PAGE_SIZE {
            return self.unassociate_address(virtual_address, pm_manager, entry_may_be_deleted);
        }

        let mut processed_size = MSize::new(0);
        while processed_size < size {
            let processing_virtual_address = virtual_address + processed_size;
            let pdpte =
                self.get_target_pdpte(pm_manager, processing_virtual_address, false, false, false)?;
            if pdpte.is_huge() {
                if !pdpte.is_present() {
                    return Err(PagingError::EntryIsNotFound);
                }
                let huge_size = MSize::new(0x40000000);
                if (size - processed_size) < huge_size {
                    return Err(PagingError::EntryIsNotFound); /* FIX: to better error name*/
                }
                pdpte.set_present(false);
                pdpte.set_address_set(false);
                processed_size += huge_size;
                self.cleanup_page_table(processing_virtual_address, pm_manager)?;
                continue;
            }
            let pde = self.get_target_pde(
                pm_manager,
                processing_virtual_address,
                false,
                false,
                false,
                Some(pdpte),
            )?;
            if pde.is_huge() {
                if !pde.is_present() {
                    return Err(PagingError::EntryIsNotFound);
                }
                let huge_size = MSize::new(0x200000);
                if (size - processed_size) < huge_size {
                    return Err(PagingError::EntryIsNotFound); /* FIX: to better error name*/
                }
                pde.set_present(false);
                pde.set_address_set(false);
                processed_size += huge_size;
                self.cleanup_page_table(processing_virtual_address, pm_manager)?;
                continue;
            }
            let pte = self.get_target_pte(
                pm_manager,
                processing_virtual_address,
                false,
                false,
                Some(pde),
            )?;
            if !pte.is_present() {
                return Err(PagingError::EntryIsNotFound);
            }
            pte.set_present(false);
            self.cleanup_page_table(processing_virtual_address, pm_manager)?;
            processed_size += PAGE_SIZE;
        }
        return Ok(());
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
        let number_of_pde = (virtual_address.to_usize() >> (PAGE_SHIFT + 9 * 1)) & (0x1FF);
        let pml4e = &mut self.get_top_level_table()[number_of_pml4e];
        if !pml4e.is_address_set() {
            return Ok(());
        }

        let pdpte = &mut unsafe {
            &mut *(physical_address_to_direct_map(pml4e.get_address().unwrap()).to_usize()
                as *mut [PDPTE; PDPT_MAX_ENTRY])
        }[number_of_pdpte];
        if pdpte.is_present() && !pdpte.is_huge() {
            let pde = &mut unsafe {
                &mut *(physical_address_to_direct_map(pdpte.get_address().unwrap()).to_usize()
                    as *mut [PDE; PD_MAX_ENTRY])
            }[number_of_pde];
            if pde.is_present() {
                /* Try to free PT */
                if !pde.is_huge() {
                    for e in unsafe {
                        &*(physical_address_to_direct_map(pde.get_address().unwrap()).to_usize()
                            as *const [PTE; PT_MAX_ENTRY])
                    }
                    .iter()
                    {
                        if e.is_present() {
                            return Ok(());
                        }
                    }
                    if let Err(_e) = pm_manager.free(pde.get_address().unwrap(), PAGE_SIZE, false)
                    /*free PT */
                    {
                        return Err(PagingError::MemoryCacheOverflowed);
                    }
                    pde.set_present(false);
                    pde.set_address_set(false);
                }
            }
            /* Try to free PD */
            for e in unsafe {
                &*(physical_address_to_direct_map(pdpte.get_address().unwrap()).to_usize()
                    as *const [PDE; PD_MAX_ENTRY])
            }
            .iter()
            {
                if e.is_present() {
                    return Ok(());
                }
            }
            if let Err(_e) = pm_manager.free(pdpte.get_address().unwrap(), PAGE_SIZE, false)
            /* free PD */
            {
                return Err(PagingError::MemoryCacheOverflowed);
            }
            pdpte.set_present(false);
            pdpte.set_address_set(false);
        }
        /* Try to free PDPT */
        for e in unsafe {
            &*(physical_address_to_direct_map(pml4e.get_address().unwrap()).to_usize()
                as *const [PDPTE; PDPT_MAX_ENTRY])
        }
        .iter()
        {
            if e.is_present() {
                return Ok(());
            }
        }
        if let Err(_e) = pm_manager.free(pml4e.get_address().unwrap(), PAGE_SIZE, false)
        /*free PDPT*/
        {
            return Err(PagingError::MemoryCacheOverflowed);
        }
        pml4e.set_present(false);
        pml4e.set_address_set(false);

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
    /// **This function must call after [`init`], otherwise system may crash.**
    ///
    /// [`init`]: #method.init
    pub fn flush_page_table(&mut self) {
        unsafe {
            cpu::set_cr3(direct_map_to_physical_address(self.pml4).to_usize());
        }
    }

    /// Flush page table and apply new page table.
    ///
    /// This function will return PML4 address.
    /// **This function must call after [`init`], otherwise system may crash.**
    ///
    /// [`init`]: #method.init
    pub fn get_page_table_address(&self) -> PAddress {
        direct_map_to_physical_address(self.pml4)
    }

    /// Delete the paging cache of the target address and update it.
    ///
    /// This function operates invpg.
    pub fn update_page_cache(virtual_address: VAddress) {
        unsafe {
            cpu::invlpg(virtual_address.to_usize());
        }
    }

    /// Dump paging table
    ///
    /// This function shows the status of paging, it prints a lot.
    pub fn dump_table(&self, start: Option<VAddress>, end: Option<VAddress>) {
        let mut permission = (false /* writable */, false /* no_execute */);
        let mut omitted = false;
        let mut last_address = (
            VAddress::from(0), /* virtual address */
            PAddress::from(0), /* physical address */
        );
        let print_normal = |v: usize, p: usize, w: bool, e: bool, a: bool, s: &str| {
            kprintln!("Linear addresses: {:>#16X} => Physical Address: {:>#16X}, W:{:>5}, E:{:>5}, A:{:>5} {}", v, p, w, e, a, s);
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
                | (pde_count << (PAGE_SHIFT + 9 * 1))
                | (pte_count << (PAGE_SHIFT + 9 * 0));
            VAddress::new(if (address & (1 << 47)) != 0 {
                (0xffff << 48) | address
            } else {
                address
            })
        };

        let pml4_table = self.get_top_level_table();
        for (pml4_count, pml4) in pml4_table.iter().enumerate() {
            if !pml4.is_address_set() {
                continue;
            }
            let pdpt = unsafe {
                &*(physical_address_to_direct_map(pml4.get_address().unwrap()).to_usize()
                    as *const [PDPTE; PDPT_MAX_ENTRY])
            };
            for (pdpte_count, pdpte) in pdpt.iter().enumerate() {
                if !pdpte.is_address_set() {
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
                    &*(physical_address_to_direct_map(pdpte.get_address().unwrap()).to_usize()
                        as *const [PDE; PD_MAX_ENTRY])
                };
                for (pde_count, pde) in pd.iter().enumerate() {
                    if !pde.is_address_set() {
                        continue;
                    }
                    if pde.is_huge() {
                        let virtual_address =
                            calculate_virtual_address(pml4_count, pdpte_count, pde_count, 0);
                        if start.is_some() && virtual_address < start.unwrap() {
                            continue;
                        }
                        if last_address.0 + MSize::new(1 << (PAGE_SHIFT + 9 * 1)) == virtual_address
                            && last_address.1 + MSize::from(1 << (PAGE_SHIFT + 9 * 1))
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
                        &*(physical_address_to_direct_map(pde.get_address().unwrap()).to_usize()
                            as *const [PTE; PT_MAX_ENTRY])
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
                        if last_address.0 + MSize::new(1 << (PAGE_SHIFT + 9 * 0)) == virtual_address
                            && last_address.1 + MSize::new(1 << (PAGE_SHIFT + 9 * 0))
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
