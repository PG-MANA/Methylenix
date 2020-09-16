/*
 *  Fixed ACPI Description Table
 */

use super::super::INITIAL_MMAP_SIZE;

use crate::kernel::drivers::acpi::acpi_pm_timer::AcpiPmTimer;
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{Address, VAddress};

#[repr(C, packed)]
struct FADT {
    signature: [u8; 4],
    length: u32,
    major_version: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: [u8; 4],
    creator_revision: [u8; 4],
    ignore: [u8; 76 - 36],
    pm_tmr_block: u32,
    ignore2: [u8; 112 - 80],
    flags: u32,
    ignore3: [u8; 276 - 116],
}

pub struct FadtManager {
    base_address: VAddress,
    enabled: bool,
}

impl FadtManager {
    pub const fn new() -> Self {
        Self {
            base_address: VAddress::new(0),
            enabled: false,
        }
    }

    pub const SIGNATURE: [u8; 4] = ['F' as u8, 'A' as u8, 'C' as u8, 'P' as u8];
    pub fn init(&mut self, fadt_vm_address: VAddress) -> bool {
        /* bgrt_vm_address must be accessible */
        let fadt = unsafe { &*(fadt_vm_address.to_usize() as *const FADT) };
        if fadt.major_version != 1 {
            pr_err!("Not supported FADT version:{}", fadt.major_version);
        }
        let fadt_vm_address = if let Ok(a) = get_kernel_manager_cluster()
            .memory_manager
            .lock()
            .unwrap()
            .mremap_dev(
                fadt_vm_address,
                INITIAL_MMAP_SIZE.into(),
                (fadt.length as usize).into(),
            ) {
            a
        } else {
            pr_err!("Cannot reserve memory area of BGRT.");
            return false;
        };
        self.base_address = fadt_vm_address;
        self.enabled = true;
        return true;
    }

    pub fn get_acpi_pm_timer(&self) -> Option<AcpiPmTimer> {
        if self.enabled {
            let fadt = unsafe { &*(self.base_address.to_usize() as *const FADT) };
            Some(AcpiPmTimer::new(
                fadt.pm_tmr_block as usize,
                ((fadt.flags >> 8) & 1) != 0,
            ))
        } else {
            None
        }
    }
}
