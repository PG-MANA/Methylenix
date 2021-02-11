//!
//! Extended System Description Table
//!
//! This manager contains the information about Extended System Description Table(XSDT).
//! XSDT is the list of tables like MADT.

use super::table::bgrt::BgrtManager;
use super::table::dsdt::DsdtManager;
use super::table::fadt::FadtManager;
use super::table::madt::MadtManager;
use super::INITIAL_MMAP_SIZE;

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{
    Address, MSize, MemoryPermissionFlags, PAddress, VAddress,
};

pub struct XsdtManager {
    base_address: VAddress,
    enabled: bool,
    /* Essential Managers */
    fadt_manager: FadtManager,
    madt_manager: MadtManager,
    dsdt_manager: DsdtManager,
    /* Optional Managers */
    bgrt_manager: BgrtManager,
}

impl XsdtManager {
    pub const fn new() -> Self {
        XsdtManager {
            base_address: VAddress::new(0),
            enabled: false,
            bgrt_manager: BgrtManager::new(),
            fadt_manager: FadtManager::new(),
            madt_manager: MadtManager::new(),
            dsdt_manager: DsdtManager::new(),
        }
    }

    pub fn init(&mut self, xsdt_physical_address: PAddress) -> bool {
        let xsdt_vm_address = if let Ok(a) = get_kernel_manager_cluster()
            .memory_manager
            .lock()
            .unwrap()
            .mmap_dev(
                xsdt_physical_address,
                MSize::new(INITIAL_MMAP_SIZE),
                MemoryPermissionFlags::rodata(),
            ) {
            a
        } else {
            pr_err!("Cannot reserve memory area of XSDT.");
            return false;
        };

        if unsafe { *(xsdt_vm_address.to_usize() as *const [u8; 4]) }
            != ['X' as u8, 'S' as u8, 'D' as u8, 'T' as u8]
        {
            pr_err!("XSDT Signature is not correct.");
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
            pr_err!("Cannot remap memory area of XSDT.");
            return false;
        };
        self.base_address = xsdt_vm_address;
        self.enabled = true;

        let mut index = 0;
        while let Some(entry_physical_address) = self.get_entry(index) {
            let v_address = if let Ok(a) = get_kernel_manager_cluster()
                .memory_manager
                .lock()
                .unwrap()
                .mmap_dev(
                    entry_physical_address,
                    MSize::new(INITIAL_MMAP_SIZE),
                    MemoryPermissionFlags::rodata(),
                ) {
                a
            } else {
                pr_err!("Cannot reserve memory area of ACPI Table.");
                return false;
            };
            drop(entry_physical_address); /* Avoid using it */
            match unsafe { *(v_address.to_usize() as *const [u8; 4]) } {
                BgrtManager::SIGNATURE => {
                    if !self.bgrt_manager.init(v_address) {
                        pr_err!("Cannot init BGRT Manager.");
                        return false;
                    }
                }
                FadtManager::SIGNATURE => {
                    if !self.fadt_manager.init(v_address) {
                        pr_err!("Cannot init FADT Manager.");
                        return false;
                    }
                }
                MadtManager::SIGNATURE => {
                    if !self.madt_manager.init(v_address) {
                        pr_err!("Cannot init MADT Manager.");
                        return false;
                    }
                }
                DsdtManager::SIGNATURE => {
                    if !self.dsdt_manager.init(v_address) {
                        pr_err!("Cannot init DSDT Manager.");
                        return false;
                    }
                }
                _ => {
                    //
                }
            };
            pr_info!(
                "{}",
                core::str::from_utf8(unsafe { &*(v_address.to_usize() as *const [u8; 4]) })
                    .unwrap_or("????")
            );
            index += 1;
        }

        if !self.dsdt_manager.is_enabled() {
            let v_address = if let Ok(a) = get_kernel_manager_cluster()
                .memory_manager
                .lock()
                .unwrap()
                .mmap_dev(
                    self.fadt_manager.get_dsdt_address().unwrap(),
                    MSize::new(INITIAL_MMAP_SIZE),
                    MemoryPermissionFlags::rodata(),
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

    pub fn get_bgrt_manager(&self) -> &BgrtManager {
        &self.bgrt_manager
    }

    pub fn get_fadt_manager(&self) -> &FadtManager {
        &self.fadt_manager
    }

    pub fn get_madt_manager(&self) -> &MadtManager {
        &self.madt_manager
    }

    pub fn get_dsdt_manager(&self) -> &DsdtManager {
        &self.dsdt_manager
    }

    fn get_length(&self) -> Option<usize> {
        if self.enabled {
            Some({ unsafe { *((self.base_address.to_usize() + 4) as *const u32) } } as usize)
        } else {
            None
        }
    }

    fn get_entry(&self, index: usize) -> Option<PAddress> {
        if self.enabled {
            if (self.get_length().unwrap() - 0x24) >> 3 > index {
                Some(PAddress::from(unsafe {
                    *((self.base_address.to_usize() + 0x24 + index * 8) as *const u64)
                } as usize))
            } else {
                None
            }
        } else {
            None
        }
    }
}
