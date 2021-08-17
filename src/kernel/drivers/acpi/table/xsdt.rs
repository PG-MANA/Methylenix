//!
//! Extended System Description Table
//!
//! This manager contains the information about Extended System Description Table(XSDT).
//! XSDT is the list of tables like MADT.

use super::bgrt::BgrtManager;
use super::dsdt::DsdtManager;
use super::fadt::FadtManager;
use super::madt::MadtManager;
use super::ssdt::SsdtManager;
use crate::kernel::drivers::acpi::INITIAL_MMAP_SIZE;

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{
    Address, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress, VAddress,
};

pub struct XsdtManager {
    base_address: VAddress,
    /* Essential Managers */
    fadt_manager: FadtManager,
    dsdt_manager: DsdtManager,
}

impl XsdtManager {
    pub const fn new() -> Self {
        Self {
            base_address: VAddress::new(0),
            fadt_manager: FadtManager::new(),
            dsdt_manager: DsdtManager::new(),
        }
    }

    pub fn init(&mut self, xsdt_physical_address: PAddress) -> bool {
        let xsdt_vm_address = if let Ok(a) = get_kernel_manager_cluster()
            .memory_manager
            .lock()
            .unwrap()
            .mmap(
                xsdt_physical_address,
                MSize::new(INITIAL_MMAP_SIZE),
                MemoryPermissionFlags::rodata(),
                MemoryOptionFlags::PRE_RESERVED
                    | MemoryOptionFlags::MEMORY_MAP
                    | MemoryOptionFlags::DO_NOT_FREE_PHYSICAL_ADDRESS,
            ) {
            a
        } else {
            pr_err!("Cannot map XSDT.");
            return false;
        };

        if unsafe { *(xsdt_vm_address.to_usize() as *const [u8; 4]) } != *b"XSDT" {
            pr_err!("Invalid XSDT Signature");
            return false;
        }
        if unsafe { *((xsdt_vm_address.to_usize() + 8) as *const u8) } != 1 {
            pr_err!("Not supported XSDT version");
            return false;
        }
        let xsdt_size = unsafe { *((xsdt_vm_address.to_usize() + 4) as *const u32) };
        let xsdt_vm_address = if let Ok(a) = get_kernel_manager_cluster()
            .memory_manager
            .lock()
            .unwrap()
            .mremap_dev(
                xsdt_vm_address,
                MSize::new(INITIAL_MMAP_SIZE),
                MSize::new(xsdt_size as usize),
            ) {
            a
        } else {
            pr_err!("Cannot remap XSDT.");
            return false;
        };
        self.base_address = xsdt_vm_address;

        let mut index = 0;
        while let Some(entry_physical_address) = self.get_entry(index) {
            let v_address = if let Ok(a) = get_kernel_manager_cluster()
                .memory_manager
                .lock()
                .unwrap()
                .mmap(
                    entry_physical_address,
                    MSize::new(INITIAL_MMAP_SIZE),
                    MemoryPermissionFlags::rodata(),
                    MemoryOptionFlags::PRE_RESERVED
                        | MemoryOptionFlags::MEMORY_MAP
                        | MemoryOptionFlags::DO_NOT_FREE_PHYSICAL_ADDRESS,
                ) {
                a
            } else {
                pr_err!("Cannot map ACPI Table.");
                return false;
            };
            drop(entry_physical_address); /* Avoid using it */
            match unsafe { *(v_address.to_usize() as *const [u8; 4]) } {
                FadtManager::SIGNATURE => {
                    if !self.fadt_manager.init(v_address) {
                        pr_err!("Cannot init FADT Manager.");
                        return false;
                    }
                }
                DsdtManager::SIGNATURE => {
                    if !self.dsdt_manager.init(v_address) {
                        pr_err!("Cannot init DSDT Manager.");
                        return false;
                    }
                }
                _ => { /* Skip */ }
            };
            pr_info!(
                "{}",
                core::str::from_utf8(unsafe { &*(v_address.to_usize() as *const [u8; 4]) })
                    .unwrap_or("----")
            );
            index += 1;
        }

        if !self.dsdt_manager.is_initialized() {
            let v_address = if let Ok(a) = get_kernel_manager_cluster()
                .memory_manager
                .lock()
                .unwrap()
                .mmap(
                    self.fadt_manager.get_dsdt_address(),
                    MSize::new(INITIAL_MMAP_SIZE),
                    MemoryPermissionFlags::rodata(),
                    MemoryOptionFlags::PRE_RESERVED
                        | MemoryOptionFlags::MEMORY_MAP
                        | MemoryOptionFlags::DO_NOT_FREE_PHYSICAL_ADDRESS,
                ) {
                a
            } else {
                pr_err!("Cannot reserve memory area of DSDT.");
                return false;
            };
            if !self.dsdt_manager.init(v_address) {
                pr_err!("Cannot init DSDT Manager.");
                return false;
            }
        }
        return true;
    }

    pub fn get_bgrt_manager(&self) -> Option<BgrtManager> {
        if let Some(v_address) = self.search_entry(&BgrtManager::SIGNATURE) {
            let mut bgrt_manager = BgrtManager::new();
            if bgrt_manager.init(v_address) {
                return Some(bgrt_manager);
            }
            pr_err!("Cannot init BGRT Manager.");
            if let Err(e) = get_kernel_manager_cluster()
                .memory_manager
                .lock()
                .unwrap()
                .free(v_address)
            {
                pr_warn!("Cannot free memory map of BGRT. Error: {:?}", e);
            }
        }
        return None;
    }

    pub fn get_fadt_manager(&self) -> &FadtManager {
        &self.fadt_manager
    }

    pub fn get_ssdt_manager<F>(&self, mut call_back: F) -> bool
    where
        F: FnMut(&SsdtManager) -> bool,
    {
        let memory_manager = &get_kernel_manager_cluster().memory_manager;
        let mut index = 0;
        while let Some(entry_physical_address) = self.get_entry(index) {
            let result = memory_manager.lock().unwrap().mmap(
                entry_physical_address,
                MSize::new(INITIAL_MMAP_SIZE),
                MemoryPermissionFlags::rodata(),
                MemoryOptionFlags::PRE_RESERVED
                    | MemoryOptionFlags::MEMORY_MAP
                    | MemoryOptionFlags::DO_NOT_FREE_PHYSICAL_ADDRESS,
            ); /* To drop Mutex Lock */

            if let Ok(v_address) = result {
                if unsafe { &*(v_address.to_usize() as *const [u8; 4]) } == &SsdtManager::SIGNATURE
                {
                    let mut ssdt_manager = SsdtManager::new();
                    if !ssdt_manager.init(v_address) || !call_back(&ssdt_manager) {
                        if let Err(e) = memory_manager.lock().unwrap().free(v_address) {
                            pr_warn!("Cannot Free SSDT: {:?}", e)
                        }
                        pr_err!("Failed initialization of SsdtManager.");
                        return false;
                    }
                } else {
                    if let Err(e) = memory_manager.lock().unwrap().free(v_address) {
                        pr_warn!("Cannot free an ACPI table: {:?}", e)
                    }
                }
            } else {
                pr_err!("Cannot map ACPI Table: {:?}", result.unwrap_err());
                return false;
            };
            index += 1;
        }
        return true;
    }

    pub fn get_madt_manager(&self) -> Option<MadtManager> {
        if let Some(v_address) = self.search_entry(&MadtManager::SIGNATURE) {
            let mut madt_manager = MadtManager::new();
            if madt_manager.init(v_address) {
                return Some(madt_manager);
            }
            pr_err!("Cannot init MADT Manager.");
            if let Err(e) = get_kernel_manager_cluster()
                .memory_manager
                .lock()
                .unwrap()
                .free(v_address)
            {
                pr_warn!("Cannot free memory map of MADT. Error: {:?}", e);
            }
        }
        return None;
    }

    pub fn get_dsdt_manager(&self) -> &DsdtManager {
        &self.dsdt_manager
    }

    fn get_length(&self) -> usize {
        unsafe { *((self.base_address.to_usize() + 4) as *const u32) as usize }
    }

    fn get_entry(&self, index: usize) -> Option<PAddress> {
        if (self.get_length() - 0x24) >> 3 > index {
            Some(PAddress::from(unsafe {
                *((self.base_address.to_usize() + 0x24 + index * 8) as *const u64)
            } as usize))
        } else {
            None
        }
    }

    fn search_entry(&self, signature: &[u8; 4]) -> Option<VAddress> {
        let mut memory_manager = get_kernel_manager_cluster().memory_manager.lock().unwrap();
        let mut index = 0;
        while let Some(entry_physical_address) = self.get_entry(index) {
            if let Ok(v_address) = memory_manager.mmap(
                entry_physical_address,
                MSize::new(INITIAL_MMAP_SIZE),
                MemoryPermissionFlags::rodata(),
                MemoryOptionFlags::PRE_RESERVED
                    | MemoryOptionFlags::MEMORY_MAP
                    | MemoryOptionFlags::DO_NOT_FREE_PHYSICAL_ADDRESS,
            ) {
                if unsafe { &*(v_address.to_usize() as *const [u8; 4]) } == signature {
                    return Some(v_address);
                }
                if let Err(e) = memory_manager.free(v_address) {
                    pr_warn!(
                        "Freeing memory map of ACPI Table was failed. Error: {:?}",
                        e
                    )
                }
            } else {
                pr_err!("Cannot map ACPI Table.");
                return None;
            };
            index += 1;
        }
        return None;
    }
}
