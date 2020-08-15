/*
 * Advanced Configuration and Power Interface Manager
 * Supported ACPI version 6.3
 * https://uefi.org/sites/default/files/resources/ACPI_6_3_May16.pdf
 */

pub mod acpi_pm_timer;
pub mod table;
pub mod xsdt;

use self::xsdt::XsdtManager;

pub struct AcpiManager {
    enabled: bool,
    _check_sum: u32,
    oem_id: [u8; 6],
    xsdt_manager: XsdtManager,
}

#[repr(C, packed)]
struct RSDP {
    signature: [u8; 8],
    checksum: u8,
    oem_id: [u8; 6],
    revision: u8,
    rsdt_address: u32,
    length: u32,
    xsdt_address: u64,
    ex_checksum: u32,
    reserved: [u8; 3],
}

pub const INITIAL_MMAP_SIZE: usize = 36;

impl AcpiManager {
    pub const fn new() -> Self {
        Self {
            enabled: false,
            _check_sum: 0,
            oem_id: [0; 6],
            xsdt_manager: XsdtManager::new(),
        }
    }

    pub fn init(&mut self, rsdp_ptr: usize) -> bool {
        /* rsdp_ptr is pointer of RSDP. */
        /* *rsdp_ptr must be readable. */
        let rsdp = unsafe { &*(rsdp_ptr as *const RSDP) };
        if rsdp.signature
            != [
                'R' as u8, 'S' as u8, 'D' as u8, ' ' as u8, 'P' as u8, 'T' as u8, 'R' as u8,
                ' ' as u8,
            ]
        {
            pr_err!("RSDP Signature is not correct.");
            return false;
        }
        if rsdp.revision != 2 {
            pr_err!("Not supported ACPI version");
            return false;
        }
        //ADD: checksum verification
        self.oem_id = rsdp.oem_id.clone();
        self.enabled = true;
        return self.xsdt_manager.init(rsdp.xsdt_address as usize);
    }

    pub fn get_oem_id(&self) -> Option<[u8; 6]> {
        if self.enabled {
            Some(self.oem_id)
        } else {
            None
        }
    }

    pub fn get_xsdt_manager(&self) -> &XsdtManager {
        &self.xsdt_manager
    }
}
