/*
 * Extended System Description Table Manager
 */

use super::table::bgrt::BgrtManager;
use super::table::fadt::FadtManager;
use super::INITIAL_MMAP_SIZE;

use kernel::manager_cluster::get_kernel_manager_cluster;
use kernel::memory_manager::data_type::{Address, PAddress, VAddress};
use kernel::memory_manager::MemoryPermissionFlags;

pub struct XsdtManager {
    base_address: VAddress,
    enabled: bool,
    bgrt_manager: BgrtManager,
    fadt_manager: FadtManager,
}

impl XsdtManager {
    pub const fn new() -> Self {
        XsdtManager {
            base_address: VAddress::new(0),
            enabled: false,
            bgrt_manager: BgrtManager::new(),
            fadt_manager: FadtManager::new(),
        }
    }

    pub fn init(&mut self, xsdt_physical_address: PAddress) -> bool {
        let xsdt_vm_address = if let Ok(a) = get_kernel_manager_cluster()
            .memory_manager
            .lock()
            .unwrap()
            .mmap_dev(
                xsdt_physical_address,
                INITIAL_MMAP_SIZE.into(),
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
                INITIAL_MMAP_SIZE.into(),
                (xsdt_size as usize).into(),
            ) {
            a
        } else {
            pr_err!("Cannot remap memory area of XSDT.");
            return false;
        };
        self.base_address = xsdt_vm_address;
        self.enabled = true;

        let mut count = 0usize;

        while let Some(entry_physical_address) = self.get_entry(count) {
            let v_address = if let Ok(a) = get_kernel_manager_cluster()
                .memory_manager
                .lock()
                .unwrap()
                .mmap_dev(
                    entry_physical_address,
                    INITIAL_MMAP_SIZE.into(),
                    MemoryPermissionFlags::rodata(),
                ) {
                a
            } else {
                pr_err!("Cannot reserve memory area of ACPI Table.");
                return false;
            };
            drop(entry_physical_address); /* avoid page fault */
            match unsafe { *(v_address.to_usize() as *const [u8; 4]) } {
                BgrtManager::BGRT_SIGNATURE => {
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
                _ => {
                    //
                }
            };
            use core::str;
            pr_info!(
                "{}",
                str::from_utf8(unsafe { &*(v_address.to_usize() as *const [u8; 4]) })
                    .unwrap_or("----")
            );
            count += 1;
        }
        return true;
    }

    pub fn get_bgrt_manager(&self) -> &BgrtManager {
        &self.bgrt_manager
    }

    pub fn get_fadt_manager(&self) -> &FadtManager {
        &self.fadt_manager
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
