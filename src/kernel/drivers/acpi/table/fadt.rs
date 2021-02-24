//!
//! Fixed ACPI Description Table
//!
//! This manager contains the information of FADT
//! FADT has the information about ACPI PowerManagement Timer.

use super::super::GeneralAddress;
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
    firmware_control: u32,
    dsdt_address: u32,
    reserved: u8,
    preferred_pm_profile: u8,
    sci_int: u16,
    smi_command: u32,
    acpi_enable: u8,
    acpi_disable: u8,
    s4_bios_req: u8,
    p_state_cnt: u8,
    pm1a_event_block: u32,
    pm1b_event_block: u32,
    pm1a_control_block: u32,
    pm1b_control_block: u32,
    pm2_control_block: u32,
    pm_tmr_block: u32,
    gp_event0_block: u32,
    gp_event1_block: u32,
    pm1_event_len: u8,
    pm1_control_len: u8,
    pm2_control_len: u8,
    pm_timer_len: u8,
    gp_event0_block_len: u8,
    gp_event1_block_len: u8,
    ignore2: [u8; 112 - 94],
    flags: u32,
    reset_register: [u8; 12],
    reset_value: u8,
    arm_boot_arch: u16,
    minor_version: u8,
    x_firmware_control_address: u64,
    x_dsdt_address: u64,
    x_pm1a_event_block: [u8; 12],
    x_pm1b_event_block: [u8; 12],
    x_pm1a_control_block: [u8; 12],
    x_pm1b_control_block: [u8; 12],
    x_pm2_control_block: [u8; 12],
    x_pm_tmr_block: [u8; 12],
    ignore3: [u8; 244 - 220],
    sleep_control_register: [u8; 12],
    sleep_status_register: [u8; 12],
    hypervisor_vendor_identity: u64,
}

pub struct FadtManager {
    base_address: VAddress,
    enabled: bool,
}

impl FadtManager {
    pub const SIGNATURE: [u8; 4] = *b"FACP";

    pub const fn new() -> Self {
        Self {
            base_address: VAddress::new(0),
            enabled: false,
        }
    }

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
            let address = GeneralAddress::new(&fadt.x_pm_tmr_block).address;
            Some(AcpiPmTimer::new(
                if address != 0 {
                    address as usize
                } else {
                    fadt.pm_tmr_block as usize
                },
                ((fadt.flags >> 8) & 1) != 0,
            ))
        } else {
            None
        }
    }

    pub fn get_flags(&self) -> Option<u32> {
        if self.enabled {
            let fadt = unsafe { &*(self.base_address.to_usize() as *const FADT) };
            Some(fadt.flags)
        } else {
            None
        }
    }

    pub fn get_pm1a_control_block_address(&self) -> Option<usize> {
        if self.enabled {
            let fadt = unsafe { &*(self.base_address.to_usize() as *const FADT) };
            let address = GeneralAddress::new(&fadt.x_pm1a_control_block).address;
            Some(if address != 0 {
                address as usize
            } else {
                fadt.pm1a_control_block as usize
            })
        } else {
            None
        }
    }

    pub fn get_pm1b_control_block_address(&self) -> Option<usize> {
        if self.enabled {
            let fadt = unsafe { &*(self.base_address.to_usize() as *const FADT) };
            let address = GeneralAddress::new(&fadt.x_pm1b_control_block).address;
            Some(if address != 0 {
                address as usize
            } else {
                fadt.pm1b_control_block as usize
            })
        } else {
            None
        }
    }

    pub fn get_pm1a_event_block_address(&self) -> Option<usize> {
        if self.enabled {
            let fadt = unsafe { &*(self.base_address.to_usize() as *const FADT) };
            let address = GeneralAddress::new(&fadt.x_pm1a_event_block).address;
            Some(if address != 0 {
                address as usize
            } else {
                fadt.pm1a_event_block as usize
            })
        } else {
            None
        }
    }

    pub fn get_pm1b_event_block_address(&self) -> Option<usize> {
        if self.enabled {
            let fadt = unsafe { &*(self.base_address.to_usize() as *const FADT) };
            let address = GeneralAddress::new(&fadt.x_pm1b_event_block).address;
            Some(if address != 0 {
                address as usize
            } else {
                fadt.pm1b_event_block as usize
            })
        } else {
            None
        }
    }

    pub fn get_pm1_event_block_len(&self) -> Option<usize> {
        if self.enabled {
            let fadt = unsafe { &*(self.base_address.to_usize() as *const FADT) };
            Some(fadt.pm1_event_len as _)
        } else {
            None
        }
    }

    pub fn get_sleep_control_register(&self) -> Option<usize> {
        if self.enabled {
            let fadt = unsafe { &*(self.base_address.to_usize() as *const FADT) };
            let address = GeneralAddress::new(&fadt.sleep_control_register).address;
            if address != 0 {
                return Some(address as usize);
            }
        }
        return None;
    }

    pub fn get_sci_int(&self) -> Option<u16> {
        if self.enabled {
            let fadt = unsafe { &*(self.base_address.to_usize() as *const FADT) };
            Some(fadt.sci_int)
        } else {
            None
        }
    }

    pub fn get_smi_cmd(&self) -> Option<usize> {
        if self.enabled {
            let fadt = unsafe { &*(self.base_address.to_usize() as *const FADT) };
            Some(fadt.smi_command as _)
        } else {
            None
        }
    }

    pub fn get_acpi_enable(&self) -> Option<usize> {
        if self.enabled {
            let fadt = unsafe { &*(self.base_address.to_usize() as *const FADT) };
            Some(fadt.acpi_enable as _)
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
