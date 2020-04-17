/*
 * Boot Graphics Resource Table Manager
 */

use kernel::manager_cluster::get_kernel_manager_cluster;

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
    creator_revision: [u8; 4],
    version: u16,
    status: u8,
    imapge_type: u8,
    image_address: u64,
    image_offset_y: u32,
    image_offset_x: u32,
}

pub struct BgrtManager {
    base_address: usize,
    enabled: bool,
}

impl BgrtManager {
    pub const BGRT_SIGNATURE: [u8; 4] = ['B' as u8, 'G' as u8, 'R' as u8, 'T' as u8];

    pub const fn new() -> Self {
        Self {
            base_address: 0,
            enabled: false,
        }
    }

    pub fn init(&mut self, bgrt_vm_address: usize) -> bool {
        /* bgrt_vm_address must be accessible */
        let bgrt = unsafe { &*(bgrt_vm_address as *const BGRT) };
        /*if bgrt.signature != Self::BGRTSIGNATURE {
            pr_err!("BGRT Signature is not correct.");
            return false;
        }*/
        if bgrt.version != 1 || bgrt.revision != 1 {
            pr_err!("Not supported BGRT version");
        }
        let bgrt_vm_address = if let Ok(a) = get_kernel_manager_cluster()
            .memory_manager
            .lock()
            .unwrap()
            .resize_memory_remap(bgrt_vm_address, 56)
        {
            a
        } else {
            pr_err!("Cannot reserve memory area of BGRT.");
            return false;
        };
        self.base_address = bgrt_vm_address;
        self.enabled = true;
        return true;
    }

    pub fn get_bitmap_physical_address(&self) -> Option<usize> {
        if self.enabled {
            let bgrt = unsafe { &*(self.base_address as *const BGRT) };
            if bgrt.imapge_type == 0 {
                return Some(bgrt.image_address as usize);
            }
        }
        return None;
    }

    pub fn get_image_offset(&self) -> Option<(usize /*x*/, usize /*y*/)> {
        if self.enabled {
            let bgrt = unsafe { &*(self.base_address as *const BGRT) };
            Some((bgrt.image_offset_x as usize, bgrt.image_offset_y as usize))
        } else {
            None
        }
    }
}
