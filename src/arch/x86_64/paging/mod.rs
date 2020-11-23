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
use self::PagingError::MemoryCacheRanOut;

use crate::arch::target_arch::device::cpu;

//use crate::kernel::memory_manager::physical_memory_manager::PhysicalMemoryManager;
use crate::kernel::memory_manager::{
    data_type::Address, data_type::MSize, data_type::PAddress, data_type::VAddress,
    pool_allocator::PoolAllocator, MemoryPermissionFlags,
};

/// Default Page Size, the mainly using 4KiB paging.(Type = MSize)
pub const PAGE_SIZE: MSize = MSize::from(PAGE_SIZE_USIZE);

/// Default Page Size, the mainly using 4KiB paging.(Type = usize)
pub const PAGE_SIZE_USIZE: usize = 0x1000;

/// PAGE_SIZE = 1 << PAGE_SHIFT(Type = usize)
pub const PAGE_SHIFT: usize = 12;

/// if !PAGE_MASK & address !=0 => address is not page aligned.
pub const PAGE_MASK: usize = 0xFFFF_FFFF_FFFF_F000;

/// Default page cache size for paging
pub const PAGING_CACHE_LENGTH: usize = 64;

/// Max virtual address of x86_64(Type = VAddress)
pub const MAX_VIRTUAL_ADDRESS: VAddress = VAddress::new(MAX_VIRTUAL_ADDRESS_USIZE);

/// Max virtual address of x86_64(Type = usize)
pub const MAX_VIRTUAL_ADDRESS_USIZE: usize = 0x0000_7FFF_FFFF_FFFF;

/// PageManager
///
/// This controls paging system.
/// This manager does not check if specified address is usable,
/// that should done by VirtualMemoryManager.
#[derive(Clone)]
pub struct PageManager {
    pml4: PAddress,
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
            pml4: PAddress::new(0),
            is_1gb_paging_supported: false,
        }
    }

    /// Init PageManager
    ///
    /// This function will take one page from cache_memory_list and set up it to PML4.
    /// After that, this will associate address of PML4(in the process, some pages are associated).
    pub fn init(
        &mut self,
        cache_memory_list: &mut PoolAllocator<
            [u8; PAGE_SIZE_USIZE], /* Memory Allocator for PAGE_SIZE bytes(u8 => 1 byte) */
        >,
    ) -> Result<(), PagingError> {
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
        let pml4_address = Self::get_address_from_cache(cache_memory_list)?;
        self.pml4 = pml4_address.into();
        let pml4_table =
            unsafe { &mut *(self.pml4.to_usize() as usize as *mut [PML4E; PML4_MAX_ENTRY]) };
        for pml4 in pml4_table.iter_mut() {
            pml4.init();
        }
        self.associate_address(
            cache_memory_list,
            pml4_address.into(),
            pml4_address.into(),
            MemoryPermissionFlags::data(),
        )
    }

    /// Search the target PDPTE linked with virtual_address.
    ///
    /// This function calculates the index number of the PDPTE linked with the virtual_address.
    /// If PML4 is not present, this will make PML4 (if should_create == true)
    /// or return PagingError::EntryIsNotFound.
    fn get_target_pdpte(
        &mut self,
        cache_memory_list: &mut PoolAllocator<[u8; PAGE_SIZE_USIZE]>,
        virtual_address: VAddress,
        should_set_present: bool,
        should_create_entry: bool,
    ) -> Result<&'static mut PDPTE, PagingError> {
        if (virtual_address.to_usize() & !PAGE_MASK) != 0 {
            return Err(PagingError::AddressIsNotAligned);
        }

        let pml4_table = unsafe { &mut *(self.pml4.to_usize() as *mut [PML4E; PML4_MAX_ENTRY]) };

        let number_of_pml4e = (virtual_address.to_usize() >> ((4 * 3) + 9 * 3)) & (0x1FF);
        let number_of_pdpte = (virtual_address.to_usize() >> ((4 * 3) + 9 * 2)) & (0x1FF);

        if !pml4_table[number_of_pml4e].is_address_set() {
            if !should_create_entry {
                return Err(PagingError::EntryIsNotFound);
            }
            let pdpt_address = Self::get_address_from_cache(cache_memory_list)?;
            let temp_pdpt = unsafe { &mut *(pdpt_address as *mut [PDPTE; PDPT_MAX_ENTRY]) };
            for entry in temp_pdpt.iter_mut() {
                entry.init();
            }
            pml4_table[number_of_pml4e].set_address(pdpt_address.into());
        }
        if should_set_present {
            pml4_table[number_of_pml4e].set_present(true);
        }

        let pdpte = &mut unsafe {
            &mut *(pml4_table[number_of_pml4e]
                .get_address()
                .unwrap()
                .to_usize() as *mut [PDPTE; PDPT_MAX_ENTRY])
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
    /// If PML4 or PDPTE is not present, this will make them (if should_create == true)
    /// or return PagingError::EntryIsNotFound.
    ///
    /// [`get_target_pdpte`]: #method.get_target_pdpte
    fn get_target_pde(
        &mut self,
        cache_memory_list: &mut PoolAllocator<[u8; PAGE_SIZE_USIZE]>,
        virtual_address: VAddress,
        should_set_present: bool,
        should_create_entry: bool,
        pdpte: Option<&'static mut PDPTE>,
    ) -> Result<&'static mut PDE, PagingError> {
        let number_of_pde = (virtual_address.to_usize() >> ((4 * 3) + 9 * 1)) & (0x1FF);

        let pdpte = pdpte.unwrap_or(self.get_target_pdpte(
            cache_memory_list,
            virtual_address,
            should_set_present,
            should_create_entry,
        )?);

        if !pdpte.is_address_set() {
            if !should_create_entry {
                return Err(PagingError::EntryIsNotFound);
            }
            let pd_address = Self::get_address_from_cache(cache_memory_list)?;
            let temp_pd = unsafe { &mut *(pd_address as *mut [PDE; PD_MAX_ENTRY]) };
            for entry in temp_pd.iter_mut() {
                entry.init();
            }
            pdpte.set_address(pd_address.into());
        }
        let pde = &mut unsafe {
            &mut *(pdpte.get_address().unwrap().to_usize() as *mut [PDE; PD_MAX_ENTRY])
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
    /// If PML4, PDPTE, or PDE is not present, this will make them (if should_create == true)
    /// or return PagingError::EntryIsNotFound.
    ///
    /// [`get_target_pde`]: #method.get_target_pde
    fn get_target_pte(
        &mut self,
        cache_memory_list: &mut PoolAllocator<[u8; PAGE_SIZE_USIZE]>,
        virtual_address: VAddress,
        should_set_present: bool,
        should_create_entry: bool,
        pde: Option<&'static mut PDE>,
    ) -> Result<&'static mut PTE, PagingError> {
        let number_of_pte = (virtual_address.to_usize() >> ((4 * 3) + 9 * 0)) & (0x1FF);

        let pde = pde.unwrap_or(self.get_target_pde(
            cache_memory_list,
            virtual_address,
            should_set_present,
            should_create_entry,
            None,
        )?);
        if !pde.is_address_set() {
            if !should_create_entry {
                return Err(PagingError::EntryIsNotFound);
            }
            let pt_address = Self::get_address_from_cache(cache_memory_list)?;
            let temp_pt = unsafe { &mut *(pt_address as *mut [PTE; PT_MAX_ENTRY]) };
            for entry in temp_pt.iter_mut() {
                entry.init();
            }
            pde.set_address(pt_address.into());
        }
        Ok(&mut unsafe {
            &mut *(pde.get_address().unwrap().to_usize() as *mut [PTE; PT_MAX_ENTRY])
        }[number_of_pte])
    }

    /// Search the target ;aging entry linked with virtual_address.
    ///
    /// This function calculates the index number of the PDPTE, PDE, and PTE linked with the virtual_address.
    /// If huge bit is present(means 1GB or 2MB paging is enabled), return with terminal page entry.
    /// If PML4 or PDPTE or PDE is not present, this will make them (if should_create == true)
    /// or return PagingError::EntryIsNotFound.
    fn get_target_paging_entry(
        &mut self,
        cache_memory_list: &mut PoolAllocator<[u8; PAGE_SIZE_USIZE]>,
        virtual_address: VAddress,
        should_set_present: bool,
        should_create_entry: bool,
    ) -> Result<&'static mut dyn PagingEntry, PagingError> {
        let pdpte = self.get_target_pdpte(
            cache_memory_list,
            virtual_address,
            should_set_present,
            should_create_entry,
        )?;
        if pdpte.is_huge() {
            return Ok(pdpte);
        }
        let pde = self.get_target_pde(
            cache_memory_list,
            virtual_address,
            should_set_present,
            should_create_entry,
            Some(pdpte),
        )?;
        if pde.is_huge() {
            return Ok(pde);
        }
        let pte = self.get_target_pte(
            cache_memory_list,
            virtual_address,
            should_set_present,
            should_create_entry,
            Some(pde),
        )?;
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
        &mut self,
        cache_memory_list: &mut PoolAllocator<[u8; PAGE_SIZE_USIZE]>,
        physical_address: PAddress,
        virtual_address: VAddress,
        permission: MemoryPermissionFlags,
    ) -> Result<(), PagingError> {
        if ((physical_address.to_usize() & !PAGE_MASK) != 0)
            || ((virtual_address.to_usize() & !PAGE_MASK) != 0)
        {
            return Err(PagingError::AddressIsNotAligned);
        }
        let pte = self.get_target_pte(cache_memory_list, virtual_address, true, true, None)?;
        pte.init();
        pte.set_address(physical_address);
        pte.set_no_execute(!permission.execute());
        pte.set_writable(permission.write());
        pte.set_user_accessible(permission.user_access());
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
        &mut self,
        cache_memory_list: &mut PoolAllocator<[u8; PAGE_SIZE_USIZE]>,
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
                cache_memory_list,
                physical_address,
                virtual_address,
                permission,
            );
        }

        let mut processed_size = MSize::from(0);
        while processed_size <= size {
            let processing_virtual_address = virtual_address + processed_size;
            let processing_physical_address = physical_address + processed_size;
            let number_of_pde =
                (processing_virtual_address.to_usize() >> ((4 * 3) + 9 * 1)) & (0x1FF);
            let number_of_pte =
                (processing_virtual_address.to_usize() >> ((4 * 3) + 9 * 0)) & (0x1FF);

            if number_of_pde == 0
                && number_of_pte == 0
                && (size - processed_size) >= MSize::from(1024 * 1024 * 1024)
                && self.is_1gb_paging_supported
            {
                /* try to apply 1GB paging */
                let pdpte = self.get_target_pdpte(
                    cache_memory_list,
                    processing_virtual_address,
                    false,
                    true,
                )?;
                /* check if PDPTE is used */
                if !pdpte.is_present() {
                    /* PDPTE is free, we can use 1GB paging! */
                    pdpte.init();
                    pdpte.set_huge(true);
                    pdpte.set_no_execute(!permission.execute());
                    pdpte.set_writable(permission.write());
                    pdpte.set_user_accessible(permission.user_access());
                    pdpte.set_address(processing_physical_address);
                    pdpte.set_present(true);
                    processed_size += MSize::from(1024 * 1024 * 1024);
                    continue;
                }
            }
            /* try to apply 2MiB paging */
            if number_of_pte == 0 && (size - processed_size) >= MSize::from(2 * 1024 * 1024) {
                let pde = self.get_target_pde(
                    cache_memory_list,
                    processing_virtual_address,
                    false,
                    true,
                    None,
                )?;
                if !pde.is_present() {
                    pde.init();
                    pde.set_huge(true);
                    pde.set_no_execute(!permission.execute());
                    pde.set_writable(permission.write());
                    pde.set_user_accessible(permission.user_access());
                    pde.set_address(processing_physical_address);
                    pde.set_present(true);
                    processed_size += MSize::from(2 * 1024 * 1024);
                    continue;
                }
            }
            /* 4KiB */
            self.associate_address(
                cache_memory_list,
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
        &mut self,
        cache_memory_list: &mut PoolAllocator<[u8; PAGE_SIZE_USIZE]>,
        virtual_address: VAddress,
        permission: MemoryPermissionFlags,
    ) -> Result<(), PagingError> {
        if (virtual_address.to_usize() & !PAGE_MASK) != 0 {
            return Err(PagingError::AddressIsNotAligned);
        }
        let entry =
            self.get_target_paging_entry(cache_memory_list, virtual_address, false, false)?;
        entry.set_writable(permission.write());
        entry.set_no_execute(!permission.execute());
        entry.set_user_accessible(permission.user_access());
        Ok(())
    }

    /// Unmap virtual_address.
    ///
    /// This function searches target page entry(usually PTE) and disable present flag.
    /// After disabling, this calls [`cleanup_page_table`] to collect freed page tables.
    /// If target entry is not exists, this function will ignore it and call [`cleanup_page_table`]
    /// when entry_may_be_deleted == true, otherwise this will return PagingError:PagingError::EntryIsNotFound.
    ///
    /// This does not delete physical address and huge bit from the entry. it  disable present flag only.
    /// It helps [`cleanup_page_table`].
    ///
    /// [`cleanup_page_table`]: #method.cleanup_page_table
    pub fn unassociate_address(
        &mut self,
        virtual_address: VAddress,
        cache_memory_list: &mut PoolAllocator<[u8; PAGE_SIZE_USIZE]>,
        entry_may_be_deleted: bool,
    ) -> Result<(), PagingError> {
        match self.get_target_paging_entry(cache_memory_list, virtual_address, false, false) {
            Ok(entry) => {
                entry.set_present(false);
                self.cleanup_page_table(virtual_address, cache_memory_list)
            }
            Err(err) => {
                if err == PagingError::EntryIsNotFound && entry_may_be_deleted {
                    self.cleanup_page_table(virtual_address, cache_memory_list)
                } else {
                    Err(err)
                }
            }
        }
    }

    /// Clean up the page table.
    ///
    /// This function searches non-used page tables(PDPT, PD, PT) and puts them into cache_memory_list.
    /// Currently, this function always returns OK(()).  
    pub fn cleanup_page_table(
        &mut self,
        virtual_address: VAddress,
        cache_memory_list: &mut PoolAllocator<[u8; PAGE_SIZE_USIZE]>,
    ) -> Result<(), PagingError> {
        let number_of_pml4e = (virtual_address.to_usize() >> ((4 * 3) + 9 * 3)) & (0x1FF);
        let number_of_pdpe = (virtual_address.to_usize() >> ((4 * 3) + 9 * 2)) & (0x1FF);
        let number_of_pde = (virtual_address.to_usize() >> ((4 * 3) + 9 * 1)) & (0x1FF);
        let pml4e = &mut unsafe { &mut *(self.pml4.to_usize() as *mut [PML4E; PML4_MAX_ENTRY]) }
            [number_of_pml4e];
        if !pml4e.is_address_set() {
            return Ok(());
        }

        let pdpte = &mut unsafe {
            &mut *(pml4e.get_address().unwrap().to_usize() as *mut [PDPTE; PDPT_MAX_ENTRY])
        }[number_of_pdpe];
        if pdpte.is_present() && !pdpte.is_huge() {
            let pde = &mut unsafe {
                &mut *(pdpte.get_address().unwrap().to_usize() as *mut [PDE; PD_MAX_ENTRY])
            }[number_of_pde];
            if pde.is_present() {
                /* Try to free PT */
                if !pde.is_huge() {
                    for e in unsafe {
                        &*(pde.get_address().unwrap().to_usize() as *const [PTE; PT_MAX_ENTRY])
                    }
                    .iter()
                    {
                        if e.is_present() {
                            return Ok(());
                        }
                    }
                    cache_memory_list.free_ptr(pde.get_address().unwrap().to_usize() as *mut _); /* free PT */
                    pde.set_present(false);
                }
            }
            /* Try to free PD */
            for e in
                unsafe { &*(pdpte.get_address().unwrap().to_usize() as *const [PDE; PD_MAX_ENTRY]) }
                    .iter()
            {
                if e.is_present() {
                    return Ok(());
                }
            }
            cache_memory_list.free_ptr(pdpte.get_address().unwrap().to_usize() as *mut _); /* free PD */
            pdpte.set_present(false);
        }
        /* Try to free PDPT */
        for e in
            unsafe { &*(pml4e.get_address().unwrap().to_usize() as *const [PDPTE; PDPT_MAX_ENTRY]) }
                .iter()
        {
            if e.is_present() {
                return Ok(());
            }
            cache_memory_list.free_ptr(pml4e.get_address().unwrap().to_usize() as *mut _); /*free PDPT*/
            pml4e.set_present(false);
        }
        Ok(())
    }

    /// Get 4KiB free page from allocator.
    ///
    /// This is the wrapper.
    /// the page is direct mapped.
    fn get_address_from_cache(
        allocator: &mut PoolAllocator<[u8; PAGE_SIZE_USIZE]>,
    ) -> Result<usize, PagingError> {
        if let Ok(a) = allocator.alloc_ptr() {
            Ok(a as usize)
        } else {
            Err(MemoryCacheRanOut)
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
            cpu::set_cr3(self.pml4.to_usize());
        }
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
    pub fn dump_table(&self, end: Option<VAddress>) {
        let mut permission = (false /* writable */, false /* no_execute */);
        let mut omitted = false;
        let mut last_address = (
            VAddress::from(0), /* virtual address */
            PAddress::from(0), /* physical address */
        );

        let pml4_table = unsafe { &*(self.pml4.to_usize() as *const [PML4E; PML4_MAX_ENTRY]) };
        for pml4 in pml4_table.iter() {
            if !pml4.is_address_set() {
                continue;
            }
            let pdpt = unsafe {
                &*(pml4.get_address().unwrap().to_usize() as *const [PDPTE; PDPT_MAX_ENTRY])
            };
            for (pdpte_count, pdpte) in pdpt.iter().enumerate() {
                if !pdpte.is_address_set() {
                    continue;
                }
                if pdpte.is_huge() {
                    let virtual_address = VAddress::from(0x40000000 * pdpte_count);

                    if last_address.0 + MSize::from(0x40000000) == virtual_address
                        && last_address.1 + MSize::from(0x40000000) == pdpte.get_address().unwrap()
                        && permission.0 == pdpte.is_writable()
                        && permission.1 == pdpte.is_no_execute()
                    {
                        last_address.0 = virtual_address;
                        last_address.1 = pdpte.get_address().unwrap();
                        omitted = true;
                        continue;
                    }
                    if omitted {
                        kprintln!(
                            " ~{:#X}: {:#X}",
                            last_address.0.to_usize(),
                            last_address.1.to_usize()
                        );
                        omitted = false;
                    }
                    kprintln!(
                        "{:#X}: {:#X} W:{}, EXE:{}, A:{} 1G",
                        virtual_address.to_usize(),
                        pdpte.get_address().unwrap().to_usize(),
                        pdpte.is_writable(),
                        !pdpte.is_no_execute(),
                        pdpte.is_accessed()
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
                    &*(pdpte.get_address().unwrap().to_usize() as *const [PDE; PD_MAX_ENTRY])
                };
                for (pde_count, pde) in pd.iter().enumerate() {
                    if !pde.is_address_set() {
                        continue;
                    }
                    if pde.is_huge() {
                        let virtual_address =
                            VAddress::from(0x40000000 * pdpte_count + 0x200000 * pde_count);

                        if last_address.0 + MSize::from(0x200000) == virtual_address
                            && last_address.1 + MSize::from(0x200000) == pde.get_address().unwrap()
                            && permission.0 == pde.is_writable()
                            && permission.1 == pde.is_no_execute()
                        {
                            last_address.0 = virtual_address;
                            last_address.1 = pde.get_address().unwrap();
                            omitted = true;
                            continue;
                        }
                        if omitted {
                            kprintln!(
                                " ~{:#X}: {:#X}",
                                last_address.0.to_usize(),
                                last_address.1.to_usize()
                            );
                            omitted = false;
                        }
                        kprintln!(
                            "{:#X}: {:#X} W:{}, EXE:{}, A:{} 2M",
                            virtual_address.to_usize(),
                            pde.get_address().unwrap().to_usize(),
                            pdpte.is_writable(),
                            !pdpte.is_no_execute(),
                            pdpte.is_accessed(),
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
                        &*(pde.get_address().unwrap().to_usize() as *const [PTE; PT_MAX_ENTRY])
                    };
                    for (pte_count, pte) in pt.iter().enumerate() {
                        if !pte.is_present() {
                            continue;
                        }
                        let virtual_address = VAddress::from(
                            0x40000000 * pdpte_count + 0x200000 * pde_count + 0x1000 * pte_count,
                        );
                        if last_address.0 + MSize::from(0x1000) == virtual_address
                            && last_address.1 + MSize::from(0x1000) == pte.get_address().unwrap()
                            && permission.0 == pte.is_writable()
                            && permission.1 == pte.is_no_execute()
                        {
                            last_address.0 = virtual_address;
                            last_address.1 = pte.get_address().unwrap();
                            omitted = true;
                            continue;
                        }
                        if omitted {
                            kprintln!(
                                " ~ {:#X}: {:#X}",
                                last_address.0.to_usize(),
                                last_address.1.to_usize()
                            );
                            omitted = false;
                        }
                        kprintln!(
                            "{:#X}: {:#X} W:{}, EXE:{}, A:{} 4K",
                            virtual_address.to_usize(),
                            pte.get_address().unwrap().to_usize(),
                            pte.is_writable(),
                            !pte.is_no_execute(),
                            pte.is_accessed()
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
