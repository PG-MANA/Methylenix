//!
//! Advanced Configuration and Power Interface Manager
//!
//! Supported ACPI version 6.4
//! <https://uefi.org/sites/default/files/resources/ACPI_6_3_May16.pdf>
//!

pub mod acpi_pm_timer;
pub mod aml;
pub mod table;
pub mod xsdt;

use self::aml::{AmlParser, NameString};
use self::xsdt::XsdtManager;

use crate::arch::target_arch::device::cpu::{disable_interrupt, in_byte, out_byte, out_word};

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
        if rsdp.signature != *b"RSD PTR " {
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
        return self.xsdt_manager.init((rsdp.xsdt_address as usize).into());
    }

    pub fn is_available(&self) -> bool {
        self.enabled
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

    pub fn enable_acpi(&self) -> bool {
        let smi_cmd = self
            .get_xsdt_manager()
            .get_fadt_manager()
            .get_smi_cmd()
            .unwrap();
        let enable = self
            .get_xsdt_manager()
            .get_fadt_manager()
            .get_acpi_enable()
            .unwrap();
        let pm1_a_port = self
            .get_xsdt_manager()
            .get_fadt_manager()
            .get_pm1a_control_block_address()
            .unwrap();

        if smi_cmd == 0 {
            return true;
        }
        unsafe { out_byte(smi_cmd as _, enable as _) };
        while (unsafe { in_byte(pm1_a_port as _) & 1 }) == 0 {
            core::hint::spin_loop();
        }
        return true;
    }

    fn get_sleep_state_object(aml_parser: &mut AmlParser, s: u8) -> Option<(usize, usize)> {
        if s > 5 {
            pr_err!("Invalid Sleep State {}", s);
            return None;
        }
        let name = NameString::from_array(&[[b'_', b'S', s + 0x30, 0]], true);
        if let Some(d) = aml_parser.get_data_ref_object(&name) {
            if let Some(mut iter) = d.to_int_iter() {
                let pm1_a = iter.next();
                let pm1_b = iter.next();
                if pm1_a.is_none() || pm1_b.is_none() {
                    pr_err!("Invalid _S{} object: {:?}", s, d);
                    None
                } else {
                    Some((pm1_a.unwrap(), pm1_b.unwrap()))
                }
            } else {
                pr_err!("Invalid _S{} Object: {:?}", s, d);
                None
            }
        } else {
            pr_err!("Cannot find _S{} Object", s);
            None
        }
    }

    fn enter_sleep_state(
        s: u8,
        aml_parser: &mut AmlParser,
        pm1_a: usize,
        pm1_b: usize,
        sleep_register: Option<usize>,
    ) -> bool {
        let s_obj = Self::get_sleep_state_object(aml_parser, s);
        if s_obj.is_none() {
            pr_err!("Cannot get _S{} Object.", s);
            return false;
        }
        let s_value = s_obj.unwrap();
        unsafe {
            if let Some(s_r) = sleep_register {
                out_byte(s_r as _, (((s_value.0 & 0b111) << 2) | (1 << 5)) as u8);
            } else if pm1_b != 0 {
                out_word(pm1_a as _, (((s_value.0 & 0b111) << 10) | (1 << 13)) as u16);
                out_word(pm1_b as _, (((s_value.1 & 0b111) << 10) | (1 << 13)) as u16);
            } else {
                out_word(pm1_a as _, (((s_value.0 & 0b111) << 10) | (1 << 13)) as u16);
            }
        }
        return true;
    }

    pub fn shutdown(&mut self, aml_parser: Option<&mut AmlParser>) -> ! {
        let mut default_parser = if aml_parser.is_none() {
            let mut p = self
                .get_xsdt_manager()
                .get_dsdt_manager()
                .get_aml_parser()
                .expect("Cannot get Aml Parser");
            assert!(p.init());
            Some(p)
        } else {
            None
        };

        let pm1_a_port = self
            .get_xsdt_manager()
            .get_fadt_manager()
            .get_pm1a_control_block_address()
            .expect("Cannot find PM1A Control Block");
        let pm1_b_port = self
            .get_xsdt_manager()
            .get_fadt_manager()
            .get_pm1b_control_block_address()
            .expect("Cannot find PM1B Control Block");

        let sleep_control_register = self
            .get_xsdt_manager()
            .get_fadt_manager()
            .get_sleep_control_register();
        if sleep_control_register.is_some() {
            pr_info!("Shutdown with HW reduced ACPI.");
        }

        unsafe { disable_interrupt() };

        assert!(
            Self::enter_sleep_state(
                5,
                aml_parser.or(default_parser.as_mut()).unwrap(),
                pm1_a_port,
                pm1_b_port,
                sleep_control_register
            ),
            "Cannot enter S5."
        );
        loop {
            core::hint::spin_loop()
        }
    }
}

pub struct GeneralAddress {
    pub address: u64,
    pub address_type: u8,
}

impl GeneralAddress {
    fn invalid() -> Self {
        Self {
            address: 0,
            address_type: 0x0B,
        }
    }

    pub fn new(a: &[u8; 12]) -> Self {
        use core::convert::TryInto;
        let address_type = a[0];
        if address_type >= 0x0B {
            return Self::invalid();
        }
        Self {
            address_type,
            address: u64::from_le_bytes((a[4..12]).try_into().unwrap()),
        }
    }
}
