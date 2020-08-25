/*
 * Paging Manager
 */

mod pde;
mod pdpte;
mod pml4e;
mod pte;

use self::pde::{PDE, PD_MAX_ENTRY};
use self::pdpte::{PDPTE, PDPT_MAX_ENTRY};
use self::pml4e::{PML4E, PML4_MAX_ENTRY};
use self::pte::{PTE, PT_MAX_ENTRY};
use self::PagingError::MemoryCacheRanOut;

use arch::target_arch::device::cpu;

//use kernel::memory_manager::physical_memory_manager::PhysicalMemoryManager;
use kernel::memory_manager::data_type::Address;
use kernel::memory_manager::{
    data_type::MSize, data_type::PAddress, data_type::VAddress, pool_allocator::PoolAllocator,
    MemoryPermissionFlags,
};

pub const PAGE_SIZE: usize = 0x1000;
pub const PAGE_SHIFT: usize = 12;
pub const PAGE_MASK: usize = 0xFFFFFFFF_FFFFF000;
pub const PAGING_CACHE_LENGTH: usize = 64;
pub const MAX_VIRTUAL_ADDRESS: usize = 0x00007FFF_FFFFFFFF;

#[derive(Clone)] //Test
pub struct PageManager {
    pml4: PAddress,
    /*&'static mut [PML4; PML4_MAX_ENTRY]*/
}

#[derive(Eq, PartialEq, Debug)]
pub enum PagingError {
    MemoryCacheRanOut,
    MemoryCacheOverflowed,
    EntryIsNotFound,
    AddressIsNotAligned,
    SizeIsNotAligned,
}

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
    pub const fn new() -> PageManager {
        PageManager {
            pml4: PAddress::new(0),
        }
    }

    pub fn init(
        &mut self,
        cache_memory_list: &mut PoolAllocator<
            [u8; PAGE_SIZE], /* Memory Allocator for PAGE_SIZE bytes(u8 => 1 byte) */
        >,
    ) -> Result<(), PagingError> {
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

    fn get_target_pdpte(
        &mut self,
        cache_memory_list: &mut PoolAllocator<[u8; PAGE_SIZE]>,
        linear_address: VAddress,
        should_set_present: bool,
        should_create_entry: bool,
    ) -> Result<&'static mut PDPTE, PagingError> {
        if (linear_address.to_usize() & !PAGE_MASK) != 0 {
            return Err(PagingError::AddressIsNotAligned);
        }

        let pml4_table = unsafe { &mut *(self.pml4.to_usize() as *mut [PML4E; PML4_MAX_ENTRY]) };

        let number_of_pml4e = (linear_address.to_usize() >> (4 * 3) + 9 * 3) & (0x1FF);
        let number_of_pdpte = (linear_address.to_usize() >> (4 * 3) + 9 * 2) & (0x1FF);

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

    fn get_target_pde(
        &mut self,
        cache_memory_list: &mut PoolAllocator<[u8; PAGE_SIZE]>,
        linear_address: VAddress,
        should_set_present: bool,
        should_create_entry: bool,
        pdpte: Option<&'static mut PDPTE>,
    ) -> Result<&'static mut PDE, PagingError> {
        let number_of_pde = (linear_address.to_usize() >> (4 * 3) + 9 * 1) & (0x1FF);

        let pdpte = pdpte.unwrap_or(self.get_target_pdpte(
            cache_memory_list,
            linear_address,
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

    fn get_target_pte(
        &mut self,
        cache_memory_list: &mut PoolAllocator<[u8; PAGE_SIZE]>,
        linear_address: VAddress,
        should_set_present: bool,
        should_create_entry: bool,
        pde: Option<&'static mut PDE>,
    ) -> Result<&'static mut PTE, PagingError> {
        let number_of_pte = (linear_address.to_usize() >> (4 * 3) + 9 * 0) & (0x1FF);

        let pde = pde.unwrap_or(self.get_target_pde(
            cache_memory_list,
            linear_address,
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

    fn get_target_paging_entry(
        &mut self,
        cache_memory_list: &mut PoolAllocator<[u8; PAGE_SIZE]>,
        linear_address: VAddress,
        should_set_present: bool,
        should_create_entry: bool,
    ) -> Result<&'static mut dyn PagingEntry, PagingError> {
        let pdpte = self.get_target_pdpte(
            cache_memory_list,
            linear_address,
            should_set_present,
            should_create_entry,
        )?;
        if pdpte.is_huge() {
            return Ok(pdpte);
        }
        let pde = self.get_target_pde(
            cache_memory_list,
            linear_address,
            should_set_present,
            should_create_entry,
            Some(pdpte),
        )?;
        if pde.is_huge() {
            return Ok(pde);
        }
        let pte = self.get_target_pte(
            cache_memory_list,
            linear_address,
            should_set_present,
            should_create_entry,
            Some(pde),
        )?;
        Ok(pte)
    }

    pub fn associate_address(
        &mut self,
        cache_memory_list: &mut PoolAllocator<[u8; PAGE_SIZE]>,
        physical_address: PAddress,
        linear_address: VAddress,
        permission: MemoryPermissionFlags,
    ) -> Result<(), PagingError> {
        /*物理アドレスと理論アドレスを結びつける*/
        if ((physical_address.to_usize() & !PAGE_MASK) != 0)
            || ((linear_address.to_usize() & !PAGE_MASK) != 0)
        {
            return Err(PagingError::AddressIsNotAligned);
        }
        let pte = self.get_target_pte(cache_memory_list, linear_address, true, true, None)?;
        pte.init();
        pte.set_address(physical_address);
        pte.set_no_execute(!permission.execute());
        pte.set_writable(permission.write());
        pte.set_user_accessible(permission.user_access());
        /* PageManager::reset_paging_local(linear_address) */
        Ok(())
    }

    pub fn associate_area(
        &mut self,
        cache_memory_list: &mut PoolAllocator<[u8; PAGE_SIZE]>,
        physical_address: PAddress,
        linear_address: VAddress,
        size: MSize,
        permission: MemoryPermissionFlags,
    ) -> Result<(), PagingError> {
        if ((physical_address.to_usize() & !PAGE_MASK) != 0)
            || ((linear_address.to_usize() & !PAGE_MASK) != 0)
        {
            return Err(PagingError::AddressIsNotAligned);
        } else if (size.to_usize() & !PAGE_MASK) != 0 {
            return Err(PagingError::SizeIsNotAligned);
        }
        if size.to_usize() == PAGE_SIZE {
            return self.associate_address(
                cache_memory_list,
                physical_address,
                linear_address,
                permission,
            );
        }

        let mut processed_size = MSize::from(0);
        while processed_size <= size {
            let processing_linear_address = linear_address + processed_size;
            let processing_physical_address = physical_address + processed_size;
            let number_of_pde = (processing_linear_address.to_usize() >> (4 * 3) + 9 * 1) & (0x1FF);
            let number_of_pte = (processing_linear_address.to_usize() >> (4 * 3) + 9 * 0) & (0x1FF);

            if number_of_pte == 0
                && number_of_pde == 0
                && (size - processed_size) >= MSize::from(1024 * 1024 * 1024)
            {
                let pdpte = self.get_target_pdpte(
                    cache_memory_list,
                    processing_linear_address,
                    false,
                    true,
                )?;
                if !pdpte.is_present() {
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
            if number_of_pte == 0 && (size - processed_size) >= MSize::from(2 * 1024 * 1024) {
                let pde = self.get_target_pde(
                    cache_memory_list,
                    processing_linear_address,
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
            self.associate_address(
                cache_memory_list,
                processing_physical_address,
                processing_linear_address,
                permission,
            )?;
            processed_size += MSize::from(PAGE_SIZE);
        }
        Ok(())
    }

    pub fn change_memory_permission(
        &mut self,
        cache_memory_list: &mut PoolAllocator<[u8; PAGE_SIZE]>,
        linear_address: VAddress,
        permission: MemoryPermissionFlags,
    ) -> Result<(), PagingError> {
        if (linear_address.to_usize() & !PAGE_MASK) != 0 {
            return Err(PagingError::AddressIsNotAligned);
        }
        let entry =
            self.get_target_paging_entry(cache_memory_list, linear_address, false, false)?;
        entry.set_writable(permission.write());
        entry.set_no_execute(!permission.execute());
        entry.set_user_accessible(permission.user_access());
        Ok(())
    }

    pub fn unassociate_address(
        &mut self,
        linear_address: VAddress,
        cache_memory_list: &mut PoolAllocator<[u8; PAGE_SIZE]>,
        entry_may_be_deleted: bool,
    ) -> Result<(), PagingError> {
        match self.get_target_paging_entry(cache_memory_list, linear_address, false, false) {
            Ok(entry) => {
                entry.set_present(false); /* Huge bitは下げないでないでおくことで 渡されたlinearアドレス*/
                self.cleanup_page_table(linear_address, cache_memory_list)
            }
            Err(err) => {
                if err == PagingError::EntryIsNotFound && entry_may_be_deleted {
                    self.cleanup_page_table(linear_address, cache_memory_list)
                } else {
                    Err(err)
                }
            }
        }
    }

    pub fn cleanup_page_table(
        &mut self,
        linear_address: VAddress,
        cache_memory_list: &mut PoolAllocator<[u8; PAGE_SIZE]>,
    ) -> Result<(), PagingError> {
        /* return needless entry to cache_memory_list */
        let number_of_pml4e = (linear_address.to_usize() >> (4 * 3) + 9 * 3) & (0x1FF);
        let number_of_pdpe = (linear_address.to_usize() >> (4 * 3) + 9 * 2) & (0x1FF);
        let number_of_pde = (linear_address.to_usize() >> (4 * 3) + 9 * 1) & (0x1FF);
        let pml4e = &mut unsafe { &mut *(self.pml4.to_usize() as *mut [PML4E; PML4_MAX_ENTRY]) }
            [number_of_pml4e];
        if !pml4e.is_address_set() {
            return Ok(());
        }

        let pdpte = &mut unsafe {
            &mut *(pml4e.get_address().unwrap().to_usize() as *mut [PDPTE; PDPT_MAX_ENTRY])
        }[number_of_pdpe];
        if pdpte.is_present() {
            if !pdpte.is_huge() {
                let pde = &mut unsafe {
                    &mut *(pdpte.get_address().unwrap().to_usize() as *mut [PDE; PD_MAX_ENTRY])
                }[number_of_pde];
                if pde.is_present() {
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
                        cache_memory_list.free_ptr(pde.get_address().unwrap().to_usize() as *mut _); /*free PT*/
                        pde.set_present(false);
                    }
                }
                for e in unsafe {
                    &*(pdpte.get_address().unwrap().to_usize() as *const [PDE; PD_MAX_ENTRY])
                }
                .iter()
                {
                    if e.is_present() {
                        return Ok(());
                    }
                }
                cache_memory_list.free_ptr(pdpte.get_address().unwrap().to_usize() as *mut _); /*free PD*/
                pdpte.set_present(false);
            }
        }
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

    fn get_address_from_cache(
        allocator: &mut PoolAllocator<[u8; PAGE_SIZE]>,
    ) -> Result<usize, PagingError> {
        if let Ok(a) = allocator.alloc_ptr() {
            Ok(a as usize)
        } else {
            Err(MemoryCacheRanOut)
        }
    }

    pub fn reset_paging(&mut self) {
        unsafe {
            cpu::set_cr3(self.pml4.to_usize());
        }
    }

    pub fn reset_paging_local(address: VAddress) {
        unsafe {
            cpu::invlpg(address.into());
        }
    }

    pub fn dump_table(&self, end: Option<VAddress>) {
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
                    let linear_address = VAddress::from(0x40000000 * pdpte_count);
                    kprintln!(
                        "{:#X}: {:#X} W:{}, EXE:{}, A:{} 1G",
                        linear_address.to_usize(),
                        pdpte.get_address().unwrap().to_usize(),
                        pdpte.is_writable(),
                        !pdpte.is_no_execute(),
                        pdpte.is_accessed()
                    );
                    if end.is_some() && linear_address >= end.unwrap() {
                        return;
                    }
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
                        let linear_address =
                            VAddress::from(0x40000000 * pdpte_count + 0x200000 * pde_count);
                        kprintln!(
                            "{:#X}: {:#X} W:{}, EXE:{}, A:{} 2M",
                            linear_address.to_usize(),
                            pde.get_address().unwrap().to_usize(),
                            pdpte.is_writable(),
                            !pdpte.is_no_execute(),
                            pdpte.is_accessed(),
                        );
                        if end.is_some() && linear_address >= end.unwrap() {
                            return;
                        }
                        continue;
                    }
                    let pt = unsafe {
                        &*(pde.get_address().unwrap().to_usize() as *const [PTE; PT_MAX_ENTRY])
                    };
                    for (pte_count, pte) in pt.iter().enumerate() {
                        if !pte.is_present() {
                            continue;
                        }
                        let linear_address = VAddress::from(
                            0x40000000 * pdpte_count + 0x200000 * pde_count + 0x1000 * pte_count,
                        );
                        kprintln!(
                            "{:#X}: {:#X} W:{}, EXE:{}, A:{} 4K",
                            linear_address.to_usize(),
                            pte.get_address().unwrap().to_usize(),
                            pte.is_writable(),
                            !pte.is_no_execute(),
                            pte.is_accessed()
                        );
                        if end.is_some() && linear_address >= end.unwrap() {
                            return;
                        }
                    }
                }
            }
        }
    }
}
