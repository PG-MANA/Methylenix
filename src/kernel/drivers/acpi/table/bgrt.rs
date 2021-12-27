//!
//! Boot Graphics Resource Table Manager
//!
//! This manager contains the information of BGRT.
//! BGRT is usually vendor logo.

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{Address, PAddress, VAddress};

#[repr(C, packed)]
struct BGRT {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: [u8; 4],
    creator_revision: u32,
    version: u16,
    status: u8,
    image_type: u8,
    image_address: u64,
    image_offset_x: u32,
    image_offset_y: u32,
}

pub struct BgrtManager {
    base_address: VAddress,
}

impl BgrtManager {
    pub const SIGNATURE: [u8; 4] = *b"BGRT";

    pub const fn new() -> Self {
        Self {
            base_address: VAddress::new(0),
        }
    }

    pub fn init(&mut self, bgrt_vm_address: VAddress) -> bool {
        /* bgrt_vm_address must be accessible */
        let bgrt = unsafe { &*(bgrt_vm_address.to_usize() as *const BGRT) };
        if bgrt.version != 1 || bgrt.revision != 1 {
            pr_err!("Not supported BGRT version");
        }
        let bgrt_vm_address = remap_table!(bgrt_vm_address, bgrt.length);
        self.base_address = bgrt_vm_address;
        return true;
    }

    pub fn get_bitmap_physical_address(&self) -> Option<PAddress> {
        let bgrt = unsafe { &*(self.base_address.to_usize() as *const BGRT) };
        if bgrt.image_type == 0 {
            Some(PAddress::new(bgrt.image_address as usize))
        } else {
            None
        }
    }

    pub fn get_image_offset(&self) -> (usize /*x*/, usize /*y*/) {
        let bgrt = unsafe { &*(self.base_address.to_usize() as *const BGRT) };
        (bgrt.image_offset_x as usize, bgrt.image_offset_y as usize)
    }
}

impl Drop for BgrtManager {
    fn drop(&mut self) {
        if !self.base_address.is_zero() {
            if let Err(e) = get_kernel_manager_cluster()
                .kernel_memory_manager
                .free(self.base_address)
            {
                pr_warn!("Cannot free BGRT: {:?}", e);
            }
        }
    }
}
