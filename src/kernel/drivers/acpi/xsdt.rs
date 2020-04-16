/*
 * Extended System Description Table Manager
 */

use super::table::bgrt::BgrtManager;
use kernel::manager_cluster::get_kernel_manager_cluster;
use kernel::memory_manager::MemoryPermissionFlags;

pub struct XsdtManager {
    base_address: usize,
    enabled: bool,
    bgrt_manager: BgrtManager,
}

impl XsdtManager {
    pub const fn new() -> Self {
        XsdtManager {
            base_address: 0,
            enabled: false,
            bgrt_manager: BgrtManager::new(),
        }
    }

    pub fn init(&mut self, xsdt_physical_address: usize) -> bool {
        let xsdt_vm_address = if let Some(a) = get_kernel_manager_cluster()
            .memory_manager
            .lock()
            .unwrap()
            .get_vm_address(
                xsdt_physical_address,
                MemoryPermissionFlags::rodata(),
                true,
                true,
            ) {
            a
        } else {
            pr_err!("Cannot reserve memory area of XSDT.");
            return false;
        };
        if unsafe { *(xsdt_vm_address as *const [u8; 4]) }
            != ['X' as u8, 'S' as u8, 'D' as u8, 'T' as u8]
        {
            pr_err!("XSDT Signature is not correct.");
            return false;
        }
        if unsafe { *((xsdt_vm_address + 8) as *const u8) } != 1 {
            pr_err!("Not supported XSDT version");
            return false;
        }
        self.base_address = xsdt_vm_address;
        self.enabled = true;

        let mut count = 0usize;

        while let Some(address) = self.get_entry(count) {
            let v_address = if let Some(a) = get_kernel_manager_cluster()
                .memory_manager
                .lock()
                .unwrap()
                .get_vm_address(address, MemoryPermissionFlags::rodata(), true, true)
            {
                a
            } else {
                pr_err!("Cannot reserve memory area of BGRT.");
                return false;
            };
            match unsafe { *(v_address as *const [u8; 4]) } {
                BgrtManager::BGRT_SIGNATURE => {
                    if !self.bgrt_manager.init(v_address) {
                        pr_err!("Cannot int BGRT Manager.");
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
                str::from_utf8(unsafe { &*(address as *const [u8; 4]) }).unwrap_or("----")
            );
            count += 1;
        }
        return true;
    }

    pub fn get_bgrt_manager(&self) -> &BgrtManager {
        &self.bgrt_manager
    }

    fn get_length(&self) -> Option<usize> {
        if self.enabled {
            Some({ unsafe { *((self.base_address + 4) as *const u32) } } as usize)
        } else {
            None
        }
    }

    fn get_entry(&self, index: usize) -> Option<usize> {
        if self.enabled {
            if (self.get_length().unwrap() - 0x24) >> 3 > index {
                Some(
                    { unsafe { *((self.base_address + 0x24 + index * 8) as *const u64) } } as usize,
                )
            } else {
                None
            }
        } else {
            None
        }
    }
}
