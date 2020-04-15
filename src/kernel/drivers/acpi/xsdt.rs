/*
 * Extended System Description Table Manager
 */

use arch::target_arch::paging::PAGE_SIZE;

use kernel::manager_cluster::get_kernel_manager_cluster;
use kernel::memory_manager::MemoryPermissionFlags;

pub struct XsdtManager {
    base_address: usize,
    enabled: bool,
}

impl XsdtManager {
    pub fn new() -> Self {
        XsdtManager {
            base_address: 0,
            enabled: false,
        }
    }

    pub fn init(&mut self, xsdt_address: usize) -> bool {
        if !get_kernel_manager_cluster()
            .memory_manager
            .lock()
            .unwrap()
            .reserve_memory(
                xsdt_address,
                xsdt_address,
                PAGE_SIZE, /* too big.. */
                MemoryPermissionFlags::rodata(),
                true,
                true,
            )
        {
            pr_err!("Cannot reserve memory area of XSDT.");
            // return false;
        }
        if unsafe { *(xsdt_address as *const [u8; 4]) }
            != ['X' as u8, 'S' as u8, 'D' as u8, 'T' as u8]
        {
            pr_err!("XSDT Signature is not correct.");
            return false;
        }
        if unsafe { *((xsdt_address + 8) as *const u8) } != 1 {
            pr_err!("Not supported XSDT version");
            return false;
        }
        self.base_address = xsdt_address;
        self.enabled = true;

        let mut count = 0usize;

        while let Some(address) = self.get_entry(count) {
            use core::str;
            pr_info!(
                "{}",
                str::from_utf8(unsafe { &*(address as *const [u8; 4]) }).unwrap_or("----")
            );
            count += 1;
        }
        return true;
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
