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
    ignore: [u8; 112 - 94],
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
    x_gpe0_block: [u8; 12],
    x_gpe1_block: [u8; 12],
    sleep_control_register: [u8; 12],
    sleep_status_register: [u8; 12],
    hypervisor_vendor_identity: u64,
}

pub struct FadtManager {
    base_address: VAddress,
}

impl FadtManager {
    pub const SIGNATURE: [u8; 4] = *b"FACP";

    pub const fn new() -> Self {
        Self {
            base_address: VAddress::new(0),
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

        return true;
    }

    pub fn get_acpi_pm_timer(&self) -> AcpiPmTimer {
        let fadt = unsafe { &*(self.base_address.to_usize() as *const FADT) };
        let address = GeneralAddress::new(&fadt.x_pm_tmr_block).address;
        AcpiPmTimer::new(
            if address != 0 {
                address as usize
            } else {
                fadt.pm_tmr_block as usize
            },
            ((fadt.flags >> 8) & 1) != 0,
        )
    }

    pub fn get_flags(&self) -> u32 {
        unsafe { &*(self.base_address.to_usize() as *const FADT) }.flags
    }

    pub fn get_pm1a_control_block_address(&self) -> usize {
        let fadt = unsafe { &*(self.base_address.to_usize() as *const FADT) };
        let address = GeneralAddress::new(&fadt.x_pm1a_control_block).address;
        if address != 0 {
            address as usize
        } else {
            fadt.pm1a_control_block as usize
        }
    }

    pub fn get_pm1b_control_block_address(&self) -> usize {
        let fadt = unsafe { &*(self.base_address.to_usize() as *const FADT) };
        let address = GeneralAddress::new(&fadt.x_pm1b_control_block).address;
        if address != 0 {
            address as usize
        } else {
            fadt.pm1b_control_block as usize
        }
    }

    pub fn get_pm1a_event_block_address(&self) -> usize {
        let fadt = unsafe { &*(self.base_address.to_usize() as *const FADT) };
        let address = GeneralAddress::new(&fadt.x_pm1a_event_block).address;
        if address != 0 {
            address as usize
        } else {
            fadt.pm1a_event_block as usize
        }
    }

    pub fn get_pm1b_event_block_address(&self) -> usize {
        let fadt = unsafe { &*(self.base_address.to_usize() as *const FADT) };
        let address = GeneralAddress::new(&fadt.x_pm1b_event_block).address;
        if address != 0 {
            address as usize
        } else {
            fadt.pm1b_event_block as usize
        }
    }

    pub fn get_pm1_event_block_len(&self) -> u8 {
        unsafe { &*(self.base_address.to_usize() as *const FADT) }.pm1_event_len
    }

    pub fn get_general_purpose_event_0_block(&self) -> usize {
        let fadt = unsafe { &*(self.base_address.to_usize() as *const FADT) };
        let address = GeneralAddress::new(&fadt.x_gpe0_block).address;
        if address != 0 {
            address as usize
        } else {
            fadt.gp_event0_block as usize
        }
    }

    pub fn get_general_purpose_event_0_block_len(&self) -> u8 {
        unsafe { &*(self.base_address.to_usize() as *const FADT) }.gp_event0_block_len
    }

    pub fn get_general_purpose_event_1_block_len(&self) -> u8 {
        unsafe { &*(self.base_address.to_usize() as *const FADT) }.gp_event1_block_len
    }

    pub fn get_general_purpose_event_1_block(&self) -> usize {
        let fadt = unsafe { &*(self.base_address.to_usize() as *const FADT) };
        let address = GeneralAddress::new(&fadt.x_gpe1_block).address;
        if address != 0 {
            address as usize
        } else {
            fadt.gp_event1_block as usize
        }
    }

    pub fn get_sleep_control_register(&self) -> Option<usize> {
        let fadt = unsafe { &*(self.base_address.to_usize() as *const FADT) };
        let address = GeneralAddress::new(&fadt.sleep_control_register).address;
        if address != 0 {
            Some(address as usize)
        } else {
            None
        }
    }

    pub fn get_sci_int(&self) -> u16 {
        unsafe { &*(self.base_address.to_usize() as *const FADT) }.sci_int
    }

    pub fn get_smi_cmd(&self) -> u32 {
        unsafe { &*(self.base_address.to_usize() as *const FADT) }.smi_command
    }

    pub fn get_acpi_enable(&self) -> u8 {
        unsafe { &*(self.base_address.to_usize() as *const FADT) }.acpi_enable
    }

    pub fn get_dsdt_address(&self) -> PAddress {
        let fadt = unsafe { &*(self.base_address.to_usize() as *const FADT) };
        if fadt.x_dsdt_address != 0 {
            PAddress::new(fadt.x_dsdt_address as usize)
        } else {
            PAddress::new(fadt.dsdt_address as usize)
        }
    }
}
