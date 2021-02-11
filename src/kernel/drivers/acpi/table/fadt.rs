//!
//! Fixed ACPI Description Table
//!
//! This manager contains the information of FADT
//! FADT has the information about ACPI PowerManagement Timer.

use super::super::INITIAL_MMAP_SIZE;

use crate::kernel::drivers::acpi::acpi_pm_timer::AcpiPmTimer;
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{Address, PAddress, VAddress};

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
    creator_revision: u32,
    firmware_control_address: u32,
    dsdt_address: u32,
    reserved: u8,
    preferred_pm_profile: u8,
    sci_int: u16,
    smi_command: u32,
    acpi_enable: u8,
    acpi_disable: u8,
    ignore: [u8; 76 - 54],
    pm_tmr_block: u32,
    ignore2: [u8; 112 - 80],
    flags: u32,
    reset_register: [u8; 12],
    reset_value: u8,
    arm_boot_arch: u16,
    minor_version: u8,
    x_firmware_control_address: u64,
    x_dsdt_address: u64,
    ignore3: [u8; 276 - 148],
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
        /* fadt_vm_address must be accessible */
        let fadt = unsafe { &*(fadt_vm_address.to_usize() as *const FADT) };
        if fadt.major_version > 6 {
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
            pr_err!("Cannot map memory area of FADT.");
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

    pub fn get_dsdt_address(&self) -> Option<PAddress> {
        if self.enabled {
            let fadt = unsafe { &*(self.base_address.to_usize() as *const FADT) };
            let address = if fadt.x_dsdt_address != 0 {
                fadt.x_dsdt_address as usize
            } else {
                fadt.dsdt_address as usize
            };
            Some(PAddress::new(address))
        } else {
            None
        }
    }
}
