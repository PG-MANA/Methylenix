/*
    Page Manager
*/


mod pml4;
mod pdpe;
mod pde;
mod pte;


use self::pml4::{PML4, PML4_MAX_ENTRY};
use self::pdpe::{PDPE, PDPE_MAX_ENTRY};
use self::pde::{PDE, PDE_MAX_ENTRY};
use self::pte::{PTE, PTE_MAX_ENTRY};
use arch::target_arch::device::cpu;

use kernel::memory_manager::physical_memory_manager::PhysicalMemoryManager;


pub const PAGE_SIZE: usize = 4 * 1024;
pub const PAGE_MASK: usize = 0xFFFFFFFF_FFFFF000;


#[derive(Clone)]//Test
pub struct PageManager {
    pml4: usize,
    /*&'static mut [PML4; PML4_MAX_ENTRY]*/
}

impl PageManager {
    pub fn new(memory_manager: &mut PhysicalMemoryManager) -> Option<PageManager> {
        let mut p: PageManager = PageManager { pml4: 0 };
        if p.init(memory_manager) {
            Some(p)
        } else {
            None
        }
    }

    pub const fn new_static() -> PageManager {
        PageManager {
            pml4: 0
        }
    }

    pub fn init(&mut self, memory_manager: &mut PhysicalMemoryManager) -> bool {
        if let Some(pml4_address) = memory_manager.alloc(PAGE_SIZE, true) {
            self.pml4 = pml4_address;
            let pml4_table = unsafe { &mut *(self.pml4 as *mut [PML4; PML4_MAX_ENTRY]) };
            for pml4 in pml4_table.iter_mut() {
                pml4.init();
            }
            self.associate_address(memory_manager, pml4_address, pml4_address, false, true, false);
            true
        } else {
            false
        }
    }

    fn get_target_pte(&mut self, memory_manager: &mut PhysicalMemoryManager, linear_address: usize, should_set_present: bool) -> Option<&'static mut PTE> {
        let number_of_pml4 = (linear_address >> (4 * 3) + 9 * 3) & (0x1FF);
        let number_of_pdpe = (linear_address >> (4 * 3) + 9 * 2) & (0x1FF);
        let number_of_pde = (linear_address >> (4 * 3) + 9 * 1) & (0x1FF);
        let number_of_pte = (linear_address >> (4 * 3) + 9 * 0) & (0x1FF);
        let pml4_table = unsafe { &mut *(self.pml4 as *mut [PML4; PML4_MAX_ENTRY]) };

        if !pml4_table[number_of_pml4].is_pdpe_set() {
            if let Some(address) = memory_manager.alloc(PAGE_SIZE, true) {
                let temp_pdpe = unsafe { &mut *(address as *mut [PDPE; PDPE_MAX_ENTRY]) };
                for entry in temp_pdpe.iter_mut() {
                    entry.init();
                }
                pml4_table[number_of_pml4].set_addr(address);
                self.associate_address(memory_manager, address, address, false, true, false);
            } else {
                return None;
            }
        }
        if should_set_present {
            pml4_table[number_of_pml4].set_present(true);
        }

        let pdpe = unsafe { &mut ((&mut *(pml4_table[number_of_pml4].get_addr().unwrap() as *mut [PDPE; PDPE_MAX_ENTRY]))[number_of_pdpe]) };
        if !pdpe.is_pde_set() {
            if let Some(address) = memory_manager.alloc(PAGE_SIZE, true) {
                let temp_pde = unsafe { &mut *(address as *mut [PDE; PDE_MAX_ENTRY]) };
                for entry in temp_pde.iter_mut() {
                    entry.init();
                }
                pdpe.set_addr(address);
                self.associate_address(memory_manager, address, address, false, true, false);
            } else {
                return None;
            }
        }
        if should_set_present {
            pdpe.set_present(true);
        }
        let pde = unsafe { &mut ((&mut *(pdpe.get_addr().unwrap() as *mut [PDE; PDE_MAX_ENTRY]))[number_of_pde]) };
        if !pde.is_pte_set() {
            if let Some(address) = memory_manager.alloc(PAGE_SIZE, true) {
                let temp_pte = unsafe { &mut *(address as *mut [PTE; PTE_MAX_ENTRY]) };
                for entry in temp_pte.iter_mut() {
                    entry.init();
                }
                pde.set_addr(address);
                self.associate_address(memory_manager, address, address, false, true, false);
            } else {
                return None;
            }
        }
        if should_set_present {
            pde.set_present(true);
        }

        Some(unsafe { &mut ((&mut *(pde.get_addr().unwrap() as *mut [PTE; PTE_MAX_ENTRY]))[number_of_pte]) })
    }

    pub fn associate_address(&mut self, memory_manager: &mut PhysicalMemoryManager, physical_address: usize, linear_address: usize, is_code: bool, is_writable: bool, is_user_accessible: bool) -> bool {
        /*物理アドレスと理論アドレスを結びつける*/
        if ((physical_address & !PAGE_MASK) != 0) || ((linear_address & !PAGE_MASK) != 0) {
            return false;
        }

        if let Some(pte) = self.get_target_pte(memory_manager, linear_address, true) {
            pte.set_addr(physical_address);
            pte.set_no_execute(!is_code);
            pte.set_writable(is_writable);
            pte.set_user_accessible(is_user_accessible);
            /*PageManager::reset_paging_local(linear_address)*/
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
            let pdpe_table = unsafe { &*(pml4.get_addr().unwrap() as *const [PDPE; PDPE_MAX_ENTRY]) };
            for pdpe in pdpe_table.iter() {
                if !pdpe.is_pde_set() {
                    continue;
                }
                let pde_table = unsafe { &*(pdpe.get_addr().unwrap() as *const [PDE; PDE_MAX_ENTRY]) };
                for pde in pde_table.iter() {
                    if !pde.is_pte_set() {
                        continue;
                    }
                    let pte_table = unsafe { &*(pde.get_addr().unwrap() as *const [PTE; PTE_MAX_ENTRY]) };
                    for pte in pte_table.iter() {
                        if !pte.is_present() {
                            continue;
                        }
                        println!("Address:0x{:X},WRITABLE:{},EXECUTABLE:{},ACCESSED:{}", pte.get_addr().unwrap(), pte.is_writable(), !pte.is_no_execute(), pte.is_accessed());
                        if pte.get_addr().unwrap() >= end {
                            return;
                        }
                    }
                }
            }
        }
    }
}