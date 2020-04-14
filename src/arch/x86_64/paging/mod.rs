/*
    Page Manager
*/

mod pde;
mod pdpe;
mod pml4;
mod pte;

use self::pde::{PDE, PDE_MAX_ENTRY};
use self::pdpe::{PDPE, PDPE_MAX_ENTRY};
use self::pml4::{PML4, PML4_MAX_ENTRY};
use self::pte::{PTE, PTE_MAX_ENTRY};
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
        let pml4_table = unsafe { &mut *(self.pml4 as *mut [PML4; PML4_MAX_ENTRY]) };
        for pml4 in pml4_table.iter_mut() {
            pml4.init();
        }
        self.associate_address(
            cache_memory_list,
            pml4_address,
            pml4_address,
            MemoryPermissionFlags::data(),
        )
    }

    fn get_target_pte(
        &mut self,
        cache_memory_list: &mut FreePageList,
        linear_address: usize,
        should_set_present: bool,
    ) -> Option<&'static mut PTE> {
        let number_of_pml4 = (linear_address >> (4 * 3) + 9 * 3) & (0x1FF);
        let number_of_pdpe = (linear_address >> (4 * 3) + 9 * 2) & (0x1FF);
        let number_of_pde = (linear_address >> (4 * 3) + 9 * 1) & (0x1FF);
        let number_of_pte = (linear_address >> (4 * 3) + 9 * 0) & (0x1FF);
        let pml4_table = unsafe { &mut *(self.pml4 as *mut [PML4; PML4_MAX_ENTRY]) };

        if !pml4_table[number_of_pml4].is_pdpe_set() {
            if cache_memory_list.pointer == 0 {
                return None; //should throw error
            }
            let address = cache_memory_list.list[cache_memory_list.pointer - 1];
            cache_memory_list.pointer -= 1;
            let temp_pdpe = unsafe { &mut *(address as *mut [PDPE; PDPE_MAX_ENTRY]) };
            for entry in temp_pdpe.iter_mut() {
                entry.init();
            }
            pml4_table[number_of_pml4].set_addr(address);
            pml4_table[number_of_pml4].set_pdpe_set(true);
            if !self.associate_address(
                cache_memory_list,
                address,
                address,
                MemoryPermissionFlags::data(),
            ) {
                return None;
            }
        }
        if should_set_present {
            pml4_table[number_of_pml4].set_present(true);
        }

        let pdpe = unsafe {
            &mut ((&mut *(pml4_table[number_of_pml4].get_addr().unwrap()
                as *mut [PDPE; PDPE_MAX_ENTRY]))[number_of_pdpe])
        };
        if !pdpe.is_pde_set() {
            if cache_memory_list.pointer == 0 {
                return None; //should throw error
            }
            let address = cache_memory_list.list[cache_memory_list.pointer - 1];
            cache_memory_list.pointer -= 1;
            let temp_pde = unsafe { &mut *(address as *mut [PDE; PDE_MAX_ENTRY]) };
            for entry in temp_pde.iter_mut() {
                entry.init();
            }
            pdpe.set_addr(address);
            if !self.associate_address(
                cache_memory_list,
                address,
                address,
                MemoryPermissionFlags::data(),
            ) {
                return None;
            }
        }
        if should_set_present {
            pdpe.set_present(true);
        }
        let pde = unsafe {
            &mut ((&mut *(pdpe.get_addr().unwrap() as *mut [PDE; PDE_MAX_ENTRY]))[number_of_pde])
        };
        if !pde.is_pte_set() {
            if cache_memory_list.pointer == 0 {
                return None; //should throw error
            }
            let address = cache_memory_list.list[cache_memory_list.pointer - 1];
            cache_memory_list.pointer -= 1;
            let temp_pte = unsafe { &mut *(address as *mut [PTE; PTE_MAX_ENTRY]) };
            for entry in temp_pte.iter_mut() {
                entry.init();
            }
            pde.set_addr(address);
            if !self.associate_address(
                cache_memory_list,
                address,
                address,
                MemoryPermissionFlags::data(),
            ) {
                return None;
            }
        }
        if should_set_present {
            pde.set_present(true);
        }

        Some(unsafe {
            &mut ((&mut *(pde.get_addr().unwrap() as *mut [PTE; PTE_MAX_ENTRY]))[number_of_pte])
        })
    }

    pub fn associate_address(
        &mut self,
        cache_memory_list: &mut FreePageList,
        physical_address: usize,
        linear_address: usize,
        permission: MemoryPermissionFlags,
    ) -> bool {
        /*物理アドレスと理論アドレスを結びつける*/
        if ((physical_address & !PAGE_MASK) != 0) || ((linear_address & !PAGE_MASK) != 0) {
            return false;
        }

        if let Some(pte) = self.get_target_pte(cache_memory_list, linear_address, true) {
            pte.set_addr(physical_address);
            pte.set_no_execute(!permission.execute);
            pte.set_writable(permission.write);
            pte.set_user_accessible(permission.user_access);
            /*PageManager::reset_paging_local(linear_address)*/
            true
        } else {
            if cache_memory_list.pointer == 0 {
                println!("Cached Memory runs out!!");
            }
            false
        }
    }

    pub fn change_memory_permission(
        &mut self,
        cache_memory_list: &mut FreePageList,
        linear_address: usize,
        permission: MemoryPermissionFlags,
    ) -> bool {
        if (linear_address & !PAGE_MASK) != 0 {
            return false;
        }
        if let Some(pte) = self.get_target_pte(cache_memory_list, linear_address, false) {
            pte.set_writable(permission.write);
            pte.set_no_execute(!permission.execute);
            pte.set_user_accessible(permission.user_access);
            return true;
        }
        if cache_memory_list.pointer == 0 {
            println!("Cached Memory runs out!!");
        }
        false
    }

    pub fn unassociate_address(
        &mut self,
        linear_address: usize,
        cache_memory_list: &mut FreePageList,
    ) -> bool {
        if let Some(pte) = self.get_target_pte(cache_memory_list, linear_address, false) {
            //pte.set_addr(0);
            pte.set_present(false);
            //When remove page table , free address of pte and pde...
            true
        } else {
            false
        }
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

    pub fn dump_table(&self, end: usize) {
        let pml4_table = unsafe { &*(self.pml4 as *const [PML4; PML4_MAX_ENTRY]) };
        for pml4 in pml4_table.iter() {
            if !pml4.is_pdpe_set() {
                continue;
            }
            let pdpe_table =
                unsafe { &*(pml4.get_addr().unwrap() as *const [PDPE; PDPE_MAX_ENTRY]) };
            for pdpe in pdpe_table.iter() {
                if !pdpe.is_pde_set() {
                    continue;
                }
                let pde_table =
                    unsafe { &*(pdpe.get_addr().unwrap() as *const [PDE; PDE_MAX_ENTRY]) };
                for pde in pde_table.iter() {
                    if !pde.is_pte_set() {
                        continue;
                    }
                    let pte_table =
                        unsafe { &*(pde.get_addr().unwrap() as *const [PTE; PTE_MAX_ENTRY]) };
                    for pte in pte_table.iter() {
                        if !pte.is_present() {
                            continue;
                        }
                        println!(
                            "Address: 0x{:X} PM W:{}, EXE:{}, A:{}",
                            pte.get_addr().unwrap(),
                            pte.is_writable(),
                            !pte.is_no_execute(),
                            pte.is_accessed()
                        );
                        if pte.get_addr().unwrap() >= end {
                            return;
                        }
                    }
                }
            }
        }
    }
}
