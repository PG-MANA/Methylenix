//!
//! Advanced Configuration and Power Interface Manager
//!
//! Supported ACPI version 6.4
//! <https://uefi.org/sites/default/files/resources/ACPI_6_3_May16.pdf>
//!

pub mod acpi_pm_timer;
pub mod aml;
pub mod ec;
pub mod event;
pub mod table;

use self::aml::{AmlPackage, AmlParser, AmlVariable, NameString};
use self::event::{AcpiEventManager, AcpiFixedEvent};
use self::table::dsdt::DsdtManager;
use self::table::fadt::FadtManager;
use self::table::xsdt::XsdtManager;

use crate::arch::target_arch::device::cpu::{disable_interrupt, in_byte, out_byte, out_word};

use crate::kernel::memory_manager::data_type::PAddress;

pub struct AcpiManager {
    enabled: bool,
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
        if !self
            .xsdt_manager
            .init(PAddress::new(rsdp.xsdt_address as usize))
        {
            pr_err!("Cannot init XSDT Manager.");
            return false;
        }
        self.enabled = true;
        return true;
    }

    pub fn init_acpi_event_manager(&self, event_manager: &mut AcpiEventManager) -> bool {
        if self.enabled {
            *event_manager = AcpiEventManager::new(&self.get_xsdt_manager().get_fadt_manager());
            true
        } else {
            false
        }
    }

    pub fn is_available(&self) -> bool {
        self.enabled
    }

    pub fn get_oem_id(&self) -> Option<&str> {
        use core::str::from_utf8;
        if self.enabled {
            from_utf8(&self.oem_id).ok()
        } else {
            None
        }
    }

    fn get_aml_parler(&self) -> AmlParser {
        let mut p = self.get_dsdt_manager().get_aml_parser();
        assert!(p.init());
        p
    }

    pub fn get_xsdt_manager(&self) -> &XsdtManager {
        &self.xsdt_manager
    }

    /* FADT must exists */
    pub fn get_fadt_manager(&self) -> &FadtManager {
        assert!(self.is_available());
        &self.xsdt_manager.get_fadt_manager()
    }

    /* DSDT must exists */
    pub fn get_dsdt_manager(&self) -> &DsdtManager {
        assert!(self.is_available());
        &self.xsdt_manager.get_dsdt_manager()
    }

    pub fn enable_acpi(&self) -> bool {
        let smi_cmd = self.get_fadt_manager().get_smi_cmd();
        let enable = self.get_fadt_manager().get_acpi_enable();
        let pm1_a_port = self.get_fadt_manager().get_pm1a_control_block();
        let pm1_b_port = self.get_fadt_manager().get_pm1b_control_block();

        if smi_cmd == 0 {
            /* HW reduced ACPI */
            return true;
        }
        unsafe { out_byte(smi_cmd as _, enable as _) };
        while (unsafe { in_byte(pm1_a_port as _) & 1 }) == 0 {
            core::hint::spin_loop();
        }
        if pm1_b_port != 0 {
            while (unsafe { in_byte(pm1_b_port as _) & 1 }) == 0 {
                core::hint::spin_loop();
            }
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
            Some(self.get_aml_parler())
        } else {
            None
        };

        let pm1_a_port = self.get_fadt_manager().get_pm1a_control_block();
        let pm1_b_port = self.get_fadt_manager().get_pm1b_control_block();

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

    pub fn shutdown_test(&mut self) -> ! {
        use crate::kernel::timer_manager::Timer;

        /* for debug */
        unsafe { disable_interrupt() };
        let timer = self.get_fadt_manager().get_acpi_pm_timer();
        for i in (1..=3).rev() {
            println!("System will shutdown after {}s...", i);
            for _ in 0..1000 {
                timer.busy_wait_ms(1);
            }
        }
        self.shutdown(None)
    }

    pub fn search_device(
        &mut self,
        aml_parser: Option<&mut AmlParser>,
        name: &str,
        hid: &[u8; 7],
    ) -> bool {
        let mut default_aml_parser = if aml_parser.is_none() {
            Some(self.get_aml_parler())
        } else {
            None
        };
        let aml_parser = aml_parser.or(default_aml_parser.as_mut()).unwrap();

        if let Some(_) = aml_parser.get_device(
            &NameString::from_string(name).expect("Invalid NameString"),
            hid,
        ) {
            true
        } else {
            false
        }
    }

    pub fn enable_power_button(&mut self, acpi_event_manager: &mut AcpiEventManager) -> bool {
        if (self.get_fadt_manager().get_flags() & (1 << 4)) != 0 {
            pr_info!("PowerButton is the control method power button.");
            if self.search_device(None, "\\_SB.PWRB", b"PNP0C0C") {
                pr_info!("This computer has power button.");
            }
            false
        } else {
            pr_info!("PowerButton is the fixed hardware power button.");
            acpi_event_manager.enable_fixed_event(AcpiFixedEvent::PowerButton)
        }
    }

    pub fn search_intr_number_with_evaluation_aml(
        &mut self,
        bus: u8,
        device: u8,
        int_pin: u8,
    ) -> Option<u8> {
        let debug_and_return_none = |e: Option<AmlVariable>| -> Option<u8> {
            pr_err!("Invalid PCI Routing Table: {:?}", e.unwrap());
            return None;
        };
        let mut aml_parser = self.get_aml_parler();
        let routing_table_method_name = NameString::from_array(
            &[
                [b'_', b'S', b'B', 0],
                [b'P', b'C', b'I', bus + b'0'],
                [b'_', b'P', b'R', b'T'],
            ],
            true,
        ); /* \\_SB.PCI(BusNumber)._PRT */
        let evaluation_result = aml_parser.evaluate_method(&routing_table_method_name, &[]);
        if evaluation_result.is_none() {
            pr_err!("Cannot evaluate {}.", routing_table_method_name);
            return None;
        }
        if let Some(AmlVariable::Package(vector)) = &evaluation_result {
            for element in vector.iter() {
                if let AmlPackage::Package(device_element) = element {
                    if let Some(AmlPackage::ConstData(c)) = device_element.get(0) {
                        let target = c.to_int();
                        let target_device = ((target >> 0x10) & 0xFFFF) as u16;
                        let target_function = (target & 0xFFFF) as u16;
                        if target_device != device as u16 {
                            continue;
                        }
                        if target_function != 0xFFFF || device_element.len() != 4 {
                            return debug_and_return_none(evaluation_result);
                        }
                        if let AmlPackage::ConstData(c) = device_element[1] {
                            if c.to_int() != int_pin as _ {
                                continue;
                            }
                        } else {
                            return debug_and_return_none(evaluation_result);
                        }
                        if let AmlPackage::ConstData(c) = device_element[3] {
                            if c.to_int() != 0 {
                                return Some(c.to_int() as _);
                            }
                        } else {
                            return debug_and_return_none(evaluation_result);
                        }
                        return if let AmlPackage::NameString(link_device) = &device_element[2] {
                            let link_device = link_device
                                .get_element_as_name_string(link_device.len() - 1)
                                .unwrap();
                            pr_info!("Detect: {}", link_device);
                            let crs_function_name =
                                NameString::from_array(&[[b'_', b'C', b'R', b'S']], false)
                                    .get_full_name_path(&link_device.get_full_name_path(
                                        &NameString::from_array(&[[b'_', b'S', b'B', 0]], true),
                                    )); /* \\_SB.(DEVICE)._CRS */
                            let link_device_evaluation_result =
                                aml_parser.evaluate_method(&crs_function_name, &[]);
                            if link_device_evaluation_result.is_none() {
                                pr_err!("Cannot evaluate {}.", crs_function_name);
                                return None;
                            }
                            return if let Some(AmlVariable::Buffer(v)) =
                                &link_device_evaluation_result
                            {
                                let small_resource_type_tag = match v.get(0) {
                                    Some(c) => *c,
                                    None => {
                                        return debug_and_return_none(
                                            link_device_evaluation_result,
                                        );
                                    }
                                };
                                if small_resource_type_tag != 0x22
                                    && small_resource_type_tag != 0x23
                                {
                                    /* 0x04 = IRQ */
                                    pr_err!("Invalid Small Resource Type.");
                                    return debug_and_return_none(link_device_evaluation_result);
                                }

                                if v[1] != 0 {
                                    let mask = v[1];
                                    for i in 0..8 {
                                        if ((mask >> i) & 1) != 0 {
                                            return Some(i);
                                        }
                                    }
                                } else if v[2] != 0 {
                                    let mask = v[2];
                                    for i in 0..8 {
                                        if ((mask >> i) & 1) != 0 {
                                            return Some(i + 8);
                                        }
                                    }
                                }
                                return debug_and_return_none(link_device_evaluation_result);
                            } else {
                                debug_and_return_none(link_device_evaluation_result)
                            };
                        } else {
                            debug_and_return_none(evaluation_result)
                        };
                    }
                } else {
                    return debug_and_return_none(evaluation_result);
                }
            }
            pr_err!("Device Specific Table Entry was not found.");
            None
        } else {
            debug_and_return_none(evaluation_result)
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
