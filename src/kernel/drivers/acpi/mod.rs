//!
//! Advanced Configuration and Power Interface Manager
//!
//! Supported ACPI version 6.4
//! <https://uefi.org/sites/default/files/resources/ACPI_6_3_May16.pdf>
//!

pub mod aml;
pub mod device;
pub mod event;
pub mod table {
    pub mod bgrt;
    pub mod dsdt;
    pub mod fadt;
    pub mod madt;
    pub mod ssdt;
    pub mod xsdt;
}

use self::aml::aml_variable::{AmlPackage, AmlVariable};
use self::aml::{AmlInterpreter, ConstData, NameString, ResourceData};
use self::device::ec::EmbeddedController;
use self::device::AcpiDeviceManager;
use self::event::{AcpiEventManager, AcpiFixedEvent};
use self::table::dsdt::DsdtManager;
use self::table::fadt::FadtManager;
use self::table::ssdt::SsdtManager;
use self::table::xsdt::XsdtManager;

use crate::arch::target_arch::device::cpu::{
    disable_interrupt, enable_interrupt, in_byte, in_word, out_byte, out_word,
};

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::PAddress;

pub struct AcpiManager {
    enabled: bool,
    xsdt_manager: XsdtManager,
    aml_interpreter: Option<AmlInterpreter>,
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
            xsdt_manager: XsdtManager::new(),
            aml_interpreter: None,
        }
    }

    pub fn init(&mut self, rsdp_ptr: usize, device_manager: &mut AcpiDeviceManager) -> bool {
        /* rsdp_ptr is pointer of RSDP. */
        /* *rsdp_ptr must be readable. */
        let rsdp = unsafe { &*(rsdp_ptr as *const RSDP) };
        if rsdp.signature != *b"RSD PTR " {
            pr_err!("RSDP Signature is not correct.");
            return false;
        }
        if rsdp.revision != 2 {
            pr_err!("Not supported ACPI version: {}", rsdp.revision);
            return false;
        }
        //ADD: checksum verification
        if !self
            .xsdt_manager
            .init(PAddress::new(rsdp.xsdt_address as usize))
        {
            pr_err!("Cannot init XSDT Manager.");
            return false;
        }
        self.enabled = true;

        device_manager.pm_timer = self.get_fadt_manager().get_acpi_pm_timer();

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

    pub fn setup_acpi_devices(&self, device_manager: &mut AcpiDeviceManager) -> bool {
        if self.enabled {
            if self.aml_interpreter.is_none() {
                pr_err!("AML Interpreter is not available.");
                return false;
            }
            if let Some(i) = &self.aml_interpreter {
                EmbeddedController::setup(i, device_manager);
                true
            } else {
                pr_err!("AmlInterpreter is not available.");
                false
            }
        } else {
            false
        }
    }

    /// Setup Aml Interpreter
    ///
    /// This function requires memory allocation.
    pub fn setup_aml_interpreter(&mut self) -> bool {
        if self.aml_interpreter.is_some() {
            pr_info!("AmlInterpreter is already initialized.");
            return true;
        }
        use alloc::vec::Vec;
        if !self.enabled {
            return false;
        }
        let dsdt = self
            .get_dsdt_manager()
            .get_definition_block_address_and_size();
        let mut ssdt_list = Vec::new();
        if !self.xsdt_manager.get_ssdt_manager(|s: &SsdtManager| {
            ssdt_list.push(s.get_definition_block_address_and_size());
            true
        }) {
            pr_err!("Cannot get SSDT.");
            return false;
        }
        pr_info!("Detected {} SSDTs.", ssdt_list.len());
        self.aml_interpreter = AmlInterpreter::setup(dsdt, ssdt_list.as_slice());
        self.aml_interpreter.is_some()
    }

    pub fn is_available(&self) -> bool {
        self.enabled
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

    fn find_sleep_state_object(interpreter: &mut AmlInterpreter, s: u8) -> Option<(usize, usize)> {
        if s > 5 {
            pr_err!("Invalid Sleep State {}", s);
            return None;
        }
        let name = NameString::from_array(&[[b'_', b'S', s + 0x30, 0]], true);
        if let Some(d) = interpreter.get_aml_variable(&name) {
            if let AmlVariable::Package(package) = &d {
                if let Some(AmlPackage::ConstData(pm1_a)) = package.get(0) {
                    if let Some(AmlPackage::ConstData(pm1_b)) = package.get(1) {
                        return Some((pm1_a.to_int(), pm1_b.to_int()));
                    }
                }
            }
            pr_err!("Invalid _S{} Object: {:?}", s, d);
            None
        } else {
            pr_err!("Failed to find _S{} Object", s);
            None
        }
    }

    fn enter_sleep_state(
        s: u8,
        interpreter: &mut AmlInterpreter,
        pm1_a: usize,
        pm1_b: usize,
        sleep_register: Option<usize>,
    ) -> bool {
        let s_obj = Self::find_sleep_state_object(&mut interpreter.clone(), s);
        if s_obj.is_none() {
            pr_err!("Cannot get _S{} Object.", s);
            return false;
        }
        let s_value = s_obj.unwrap();
        if interpreter
            .evaluate_method(
                &NameString::from_array(&[*b"_PTS"], true),
                &[AmlVariable::ConstData(ConstData::Byte(s))],
            )
            .is_err()
        {
            pr_err!("Failed to evaluate _PTS");
        }
        unsafe {
            if let Some(s_r) = sleep_register {
                let mut status = in_byte(s_r as _);
                status &= !(0b111 << 2);
                status |= (((s_value.0 & 0b111) << 2) | (1 << 5)) as u8;
                out_byte(s_r as _, status);
            } else {
                let mut status = in_word(pm1_a as _);
                status &= !(0b111 << 10);
                status |= (((s_value.0 & 0b111) << 10) | (1 << 13)) as u16;
                out_word(pm1_a as _, status);
                if pm1_b != 0 {
                    let mut status = in_word(pm1_b as _);
                    status &= !(0b111 << 10);
                    status |= (((s_value.1 & 0b111) << 10) | (1 << 13)) as u16;
                    out_word(pm1_b as _, status);
                }
            }
        }
        return true;
    }

    pub fn shutdown(&mut self) -> ! {
        if self.aml_interpreter.is_none() {
            panic!("AML Interpreter is not available.");
        }

        let pm1_a_port = self.get_fadt_manager().get_pm1a_control_block();
        let pm1_b_port = self.get_fadt_manager().get_pm1b_control_block();

        let sleep_control_register = self
            .get_xsdt_manager()
            .get_fadt_manager()
            .get_sleep_control_register();
        if sleep_control_register.is_some() {
            pr_info!("Shutdown with HW reduced ACPI.");
        }

        assert!(
            Self::enter_sleep_state(
                5,
                self.aml_interpreter.as_mut().unwrap(),
                pm1_a_port,
                pm1_b_port,
                sleep_control_register
            ),
            "Cannot enter S5."
        );
        unsafe { disable_interrupt() };
        loop {
            core::hint::spin_loop()
        }
    }

    pub fn shutdown_test(&mut self) -> ! {
        use crate::kernel::timer_manager::Timer;

        /* for debug */
        unsafe { disable_interrupt() };
        if let Some(timer) = get_kernel_manager_cluster()
            .acpi_device_manager
            .get_pm_timer()
        {
            for i in (1..=3).rev() {
                println!("System will shutdown after {}s...", i);
                for _ in 0..1000 {
                    timer.busy_wait_ms(1);
                }
            }
        }
        unsafe { enable_interrupt() };
        self.shutdown()
    }

    pub fn enable_power_button(&mut self, acpi_event_manager: &mut AcpiEventManager) -> bool {
        if (self.get_fadt_manager().get_flags() & (1 << 4)) != 0 {
            pr_info!("PowerButton is the control method power button.");
            if let Some(interpreter) = &self.aml_interpreter {
                match interpreter.move_into_device(b"PNP0C0C") {
                    Ok(Some(i)) => {
                        pr_info!("This computer has power button: {}", i.get_current_scope());
                        true
                    }
                    Ok(None) => {
                        pr_info!("This computer has no power button.");
                        true
                    }
                    Err(_) => {
                        pr_info!("Cannot get power button device.");
                        false
                    }
                }
            } else {
                pr_err!("AmlInterpreter is not available.");
                false
            }
        } else {
            pr_info!("PowerButton is the fixed hardware power button.");
            acpi_event_manager.enable_fixed_event(AcpiFixedEvent::PowerButton)
        }
    }

    pub fn search_interrupt_information_with_evaluation_aml(
        &mut self,
        bus: u8,
        device: u8,
        int_pin: u8,
    ) -> Option<ResourceData> {
        let debug_and_return_none = |e: Option<AmlVariable>| -> Option<ResourceData> {
            pr_err!("Invalid PCI Routing Table: {:?}", e.unwrap());
            return None;
        };
        let mut interpreter = if let Some(i) = &self.aml_interpreter {
            i.clone()
        } else {
            pr_err!("AmlInterpreter is not available.");
            return None;
        };

        let routing_table_method_name =
            NameString::from_array(&[*b"_SB\0", [b'P', b'C', b'I', bus + b'0'], *b"_PRT"], true); /* \\_SB.PCI(BusNumber)._PRT */
        let evaluation_result = interpreter.evaluate_method(&routing_table_method_name, &[]);
        if evaluation_result.is_err() {
            pr_err!("Cannot evaluate {}.", routing_table_method_name);
            return None;
        }
        let returned_value = evaluation_result.unwrap();

        if let Some(AmlVariable::Package(vector)) = &returned_value {
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
                            return debug_and_return_none(returned_value);
                        }
                        if let AmlPackage::ConstData(c) = device_element[1] {
                            if c.to_int() != int_pin as _ {
                                continue;
                            }
                        } else {
                            return debug_and_return_none(returned_value);
                        }
                        if let AmlPackage::ConstData(c) = device_element[3] {
                            if c.to_int() != 0 {
                                return Some(ResourceData::Irq(c.to_int() as u8));
                            }
                        } else {
                            return debug_and_return_none(returned_value);
                        }
                        return if let AmlPackage::NameString(link_device) = &device_element[2] {
                            let link_device = link_device.get_last_element().unwrap();
                            pr_info!("Detect: {}", link_device);
                            let crs_function_name = NameString::from_array(&[*b"_CRS"], false)
                                .get_full_name_path(&link_device.get_full_name_path(
                                    &NameString::from_array(&[[b'_', b'S', b'B', 0]], true),
                                )); /* \\_SB.(DEVICE)._CRS */
                            let mut interpreter = self.aml_interpreter.as_ref().unwrap().clone();
                            let link_device_evaluation_result =
                                interpreter.evaluate_method(&crs_function_name, &[]);
                            if link_device_evaluation_result.is_err() {
                                pr_err!("Cannot evaluate {}.", crs_function_name);
                                return None;
                            }
                            let returned_value = link_device_evaluation_result.unwrap();

                            return if let Some(AmlVariable::Buffer(v)) = &returned_value {
                                let resource_type_tag = match v.get(0) {
                                    Some(c) => *c,
                                    None => {
                                        return debug_and_return_none(returned_value);
                                    }
                                };
                                match resource_type_tag {
                                    0x22 | 0x23 => {
                                        if v[1] != 0 {
                                            let mask = v[1];
                                            for i in 0..8 {
                                                if ((mask >> i) & 1) != 0 {
                                                    return Some(ResourceData::Irq(i));
                                                }
                                            }
                                        } else if v[2] != 0 {
                                            let mask = v[2];
                                            for i in 0..8 {
                                                if ((mask >> i) & 1) != 0 {
                                                    return Some(ResourceData::Irq(i + 8));
                                                }
                                            }
                                        }
                                        pr_err!("Invalid IRQ Resource Data.");
                                        debug_and_return_none(returned_value)
                                    }
                                    0x89 => {
                                        let length = *v.get(1).unwrap_or(&0) as u16
                                            | ((*v.get(2).unwrap_or(&0) as u16) << 8);
                                        if length < 0x06 {
                                            pr_err!("Invalid Large Resource Data Type.");
                                            return debug_and_return_none(returned_value);
                                        }
                                        if *v.get(4).unwrap_or(&0) != 1 {
                                            pr_err!("Interrupt table length must be 1.");
                                            return debug_and_return_none(returned_value);
                                        }
                                        Some(ResourceData::Interrupt(v[5] as usize))
                                    }
                                    _ => {
                                        pr_err!("Invalid Resource Data Type.");
                                        debug_and_return_none(returned_value)
                                    }
                                }
                            } else {
                                debug_and_return_none(returned_value)
                            };
                        } else {
                            debug_and_return_none(returned_value)
                        };
                    }
                } else {
                    return debug_and_return_none(returned_value);
                }
            }
            pr_err!("Device Specific Table Entry was not found.");
            None
        } else {
            debug_and_return_none(returned_value)
        }
    }

    pub fn initialize_all_devices(&mut self) -> bool {
        if let Some(mut interpreter) = self.aml_interpreter.clone() {
            match interpreter.initialize_all_devices() {
                Ok(()) => true,
                Err(()) => false,
            }
        } else {
            pr_err!("AmlInterpreter is not available.");
            false
        }
    }

    fn evaluate_query(&self, query: u8) {
        let interpreter = if let Some(i) = &self.aml_interpreter {
            i
        } else {
            pr_err!("AmlInterpreter is not available.");
            return;
        };
        if get_kernel_manager_cluster()
            .acpi_device_manager
            .ec
            .is_some()
        {
            if let Ok(Some(mut new_interpreter)) =
                interpreter.move_into_device(&EmbeddedController::HID)
            {
                drop(interpreter);
                let to_ascii = |x: u8| -> u8 {
                    if x >= 0xa {
                        x - 0xa + b'A'
                    } else {
                        x + b'0'
                    }
                };

                let query_method_name = NameString::from_array(
                    &[[b'_', b'Q', to_ascii(query >> 4), to_ascii(query & 0xf)]],
                    false,
                )
                .get_full_name_path(new_interpreter.get_current_scope());
                pr_info!("Evaluate: {}", query_method_name);
                if let Err(_) = new_interpreter.evaluate_method(&query_method_name, &[]) {
                    pr_err!("Cannot evaluate: {}", query_method_name);
                }
            }
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
