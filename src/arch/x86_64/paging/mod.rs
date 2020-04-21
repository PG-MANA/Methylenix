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
use arch::target_arch::device::cpu;

//use kernel::memory_manager::physical_memory_manager::PhysicalMemoryManager;
use kernel::memory_manager::{FreePageList, MemoryPermissionFlags};

pub const PAGE_SIZE: usize = 0x1000;
pub const PAGE_MASK: usize = 0xFFFFFFFF_FFFFF000;
pub const PAGING_CACHE_LENGTH: usize = 64;
pub const MAX_VIRTUAL_ADDRESS: usize = 0x00007FFF_FFFFFFFF;

#[derive(Clone)] //Test
pub struct PageManager {
    pml4: usize,
    /*&'static mut [PML4; PML4_MAX_ENTRY]*/
}

#[derive(Eq, PartialEq)]
pub enum PagingError {
    MemoryCacheRanOut,
    MemoryCacheOverflowed,
    EntryIsNotFound,
    AddressIsNotAligned,
}

impl PageManager {
    pub const fn new() -> PageManager {
        PageManager { pml4: 0 }
    }

    pub fn init(&mut self, cache_memory_list: &mut FreePageList) -> bool {
        if cache_memory_list.pointer == 0 {
            return false; //should throw error
        }
        let pml4_address = cache_memory_list.list[cache_memory_list.pointer - 1];
        cache_memory_list.pointer -= 1;
        self.pml4 = pml4_address;
        let pml4_table = unsafe { &mut *(self.pml4 as *mut [PML4E; PML4_MAX_ENTRY]) };
        for pml4 in pml4_table.iter_mut() {
            pml4.init();
        }
        self.associate_address(
            cache_memory_list,
            pml4_address,
            pml4_address,
            MemoryPermissionFlags::data(),
        )
        .is_ok()
    }

    fn get_target_pte(
        &mut self,
        cache_memory_list: &mut FreePageList,
        linear_address: usize,
        should_set_present: bool,
        should_create_entry: bool,
    ) -> Result<&'static mut PTE, PagingError> {
        if (linear_address & !PAGE_MASK) != 0 {
            return Err(PagingError::AddressIsNotAligned);
        }

        let number_of_pml4e = (linear_address >> (4 * 3) + 9 * 3) & (0x1FF);
        let number_of_pdpte = (linear_address >> (4 * 3) + 9 * 2) & (0x1FF);
        let number_of_pde = (linear_address >> (4 * 3) + 9 * 1) & (0x1FF);
        let number_of_pte = (linear_address >> (4 * 3) + 9 * 0) & (0x1FF);
        let pml4_table = unsafe { &mut *(self.pml4 as *mut [PML4E; PML4_MAX_ENTRY]) };

        if !pml4_table[number_of_pml4e].is_pdpt_set() {
            if !should_create_entry {
                return Err(PagingError::EntryIsNotFound);
            }
            if cache_memory_list.pointer == 0 {
                return Err(PagingError::MemoryCacheRanOut);
            }
            let pdpt_address = cache_memory_list.list[cache_memory_list.pointer - 1];
            cache_memory_list.pointer -= 1;
            let temp_pdpt = unsafe { &mut *(pdpt_address as *mut [PDPTE; PDPT_MAX_ENTRY]) };
            for entry in temp_pdpt.iter_mut() {
                entry.init();
            }
            pml4_table[number_of_pml4e].set_addr(pdpt_address);
        }
        if should_set_present {
            pml4_table[number_of_pml4e].set_present(true);
        }

        let pdpte = unsafe {
            &mut ((&mut *(pml4_table[number_of_pml4e].get_addr().unwrap()
                as *mut [PDPTE; PDPT_MAX_ENTRY]))[number_of_pdpte])
        };
        if !pdpte.is_pd_set() {
            if !should_create_entry {
                return Err(PagingError::EntryIsNotFound);
            }
            if cache_memory_list.pointer == 0 {
                return Err(PagingError::MemoryCacheRanOut);
            }
            let pd_address = cache_memory_list.list[cache_memory_list.pointer - 1];
            cache_memory_list.pointer -= 1;
            let temp_pd = unsafe { &mut *(pd_address as *mut [PDE; PD_MAX_ENTRY]) };
            for entry in temp_pd.iter_mut() {
                entry.init();
            }
            pdpte.set_addr(pd_address);
        }
        if should_set_present {
            pdpte.set_present(true);
        }
        let pde = unsafe {
            &mut ((&mut *(pdpte.get_addr().unwrap() as *mut [PDE; PD_MAX_ENTRY]))[number_of_pde])
        };
        if !pde.is_pt_set() {
            if !should_create_entry {
                return Err(PagingError::EntryIsNotFound);
            }
            if cache_memory_list.pointer == 0 {
                return Err(PagingError::MemoryCacheRanOut);
            }
            let pt_address = cache_memory_list.list[cache_memory_list.pointer - 1];
            cache_memory_list.pointer -= 1;
            let temp_pt = unsafe { &mut *(pt_address as *mut [PTE; PT_MAX_ENTRY]) };
            for entry in temp_pt.iter_mut() {
                entry.init();
            }
            pde.set_addr(pt_address);
        }
        if should_set_present {
            pde.set_present(true);
        }
        Ok(unsafe {
            &mut ((&mut *(pde.get_addr().unwrap() as *mut [PTE; PT_MAX_ENTRY]))[number_of_pte])
        })
    }

    pub fn associate_address(
        &mut self,
        cache_memory_list: &mut FreePageList,
        physical_address: usize,
        linear_address: usize,
        permission: MemoryPermissionFlags,
    ) -> Result<(), PagingError> {
        /*物理アドレスと理論アドレスを結びつける*/
        if ((physical_address & !PAGE_MASK) != 0) || ((linear_address & !PAGE_MASK) != 0) {
            return Err(PagingError::AddressIsNotAligned);
        }
        let pte = self.get_target_pte(cache_memory_list, linear_address, true, true)?;
        pte.set_addr(physical_address);
        pte.set_no_execute(!permission.execute());
        pte.set_writable(permission.write());
        pte.set_user_accessible(permission.user_access());
        /* PageManager::reset_paging_local(linear_address) */
        Ok(())
    }

    pub fn change_memory_permission(
        &mut self,
        cache_memory_list: &mut FreePageList,
        linear_address: usize,
        permission: MemoryPermissionFlags,
    ) -> Result<(), PagingError> {
        if (linear_address & !PAGE_MASK) != 0 {
            return Err(PagingError::AddressIsNotAligned);
        }
        let pte = self.get_target_pte(cache_memory_list, linear_address, false, false)?;
        pte.set_writable(permission.write());
        pte.set_no_execute(!permission.execute());
        pte.set_user_accessible(permission.user_access());
        Ok(())
    }

    pub fn unassociate_address(
        &mut self,
        linear_address: usize,
        cache_memory_list: &mut FreePageList,
        entry_may_be_deleted: bool,
    ) -> Result<(), PagingError> {
        match self.get_target_pte(cache_memory_list, linear_address, false, false) {
            Ok(pte) => {
                if cache_memory_list.pointer >= PAGING_CACHE_LENGTH {
                    Err(PagingError::MemoryCacheOverflowed)
                } else {
                    cache_memory_list.list[cache_memory_list.pointer] = pte.get_addr().unwrap();
                    cache_memory_list.pointer += 1;
                    pte.set_present(false);
                    self.cleanup_page_table(linear_address, cache_memory_list)
                }
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
        linear_address: usize,
        cache_memory_list: &mut FreePageList,
    ) -> Result<(), PagingError> {
        /* return needless entry to cache_memory_list */
        let number_of_pml4e = (linear_address >> (4 * 3) + 9 * 3) & (0x1FF);
        let number_of_pdpe = (linear_address >> (4 * 3) + 9 * 2) & (0x1FF);
        let number_of_pde = (linear_address >> (4 * 3) + 9 * 1) & (0x1FF);
        let pml4_table = unsafe { &mut *(self.pml4 as *mut [PML4E; PML4_MAX_ENTRY]) };
        if !pml4_table[number_of_pml4e].is_pdpt_set() {
            return Ok(());
        }

        let pdpte = unsafe {
            &mut ((&mut *(pml4_table[number_of_pml4e].get_addr().unwrap()
                as *mut [PDPTE; PDPT_MAX_ENTRY]))[number_of_pdpe])
        };
        let pde = unsafe {
            &mut ((&mut *(pdpte.get_addr().unwrap() as *mut [PDE; PD_MAX_ENTRY]))[number_of_pde])
        };
        if pde.is_present() {
            for e in unsafe { &*(pde.get_addr().unwrap() as *const [PTE; PT_MAX_ENTRY]) }.iter() {
                if e.is_present() {
                    return Ok(());
                }
            }
            if cache_memory_list.pointer >= PAGING_CACHE_LENGTH {
                return Err(PagingError::MemoryCacheOverflowed);
            }
            let pde_address = pde.get_addr().unwrap();
            cache_memory_list.list[cache_memory_list.pointer] = pde_address;
            cache_memory_list.pointer += 1;
            pde.set_present(false);
        }
        if pdpte.is_present() {
            for e in unsafe { &*(pdpte.get_addr().unwrap() as *const [PDE; PD_MAX_ENTRY]) }.iter() {
                if e.is_present() {
                    return Ok(());
                }
            }
            if cache_memory_list.pointer >= PAGING_CACHE_LENGTH {
                return Err(PagingError::MemoryCacheOverflowed);
            }
            let pdpe_address = pdpte.get_addr().unwrap();
            cache_memory_list.list[cache_memory_list.pointer] = pdpe_address;
            cache_memory_list.pointer += 1;
            pdpte.set_present(false);
        }
        Ok(())
    }

    pub fn reset_paging(&mut self) {
        unsafe {
            cpu::set_cr3(self.pml4);
        }
    }

    pub fn reset_paging_local(address: usize) {
        unsafe {
            cpu::invlpg(address);
        }
    }

    pub fn dump_table(&self, end: Option<usize>) {
        let pml4_table = unsafe { &*(self.pml4 as *const [PML4E; PML4_MAX_ENTRY]) };
        for pml4 in pml4_table.iter() {
            if !pml4.is_pdpt_set() {
                continue;
            }
            let pdpe_table =
                unsafe { &*(pml4.get_addr().unwrap() as *const [PDPTE; PDPT_MAX_ENTRY]) };
            for (pdpte_count, pdpe) in pdpe_table.iter().enumerate() {
                if !pdpe.is_pd_set() {
                    continue;
                }
                let pde_table =
                    unsafe { &*(pdpe.get_addr().unwrap() as *const [PDE; PD_MAX_ENTRY]) };
                for (pde_count, pde) in pde_table.iter().enumerate() {
                    if !pde.is_pt_set() {
                        continue;
                    }
                    let pte_table =
                        unsafe { &*(pde.get_addr().unwrap() as *const [PTE; PT_MAX_ENTRY]) };
                    for (pte_count, pte) in pte_table.iter().enumerate() {
                        if !pte.is_present() {
                            continue;
                        }
                        kprintln!(
                            "0x{:X}: 0x{:X} W:{}, EXE:{}, A:{}",
                            0x40000000 * pdpte_count + 0x200000 * pde_count + 0x1000 * pte_count,
                            pte.get_addr().unwrap(),
                            pte.is_writable(),
                            !pte.is_no_execute(),
                            pte.is_accessed()
                        );
                        if end.is_some() && pte.get_addr() >= end {
                            return;
                        }
                    }
                }
            }
        }
    }
}
