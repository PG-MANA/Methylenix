//!
//! ACPI Embedded Controller Driver
//!

use super::super::aml::aml_variable::AmlVariable;
use super::super::aml::AmlInterpreter;
use super::super::aml::{ConstData, NameString};
use super::super::device::AcpiDeviceManager;

use crate::arch::target_arch::device::acpi::{read_io_byte, write_io_byte};

use crate::kernel::sync::spin_lock::SpinLockFlag;

pub struct EmbeddedController {
    ec_sc: usize,
    ec_data: usize,
    gpe: Option<usize>,
    write_lock: SpinLockFlag,
}

impl EmbeddedController {
    pub const HID: [u8; 7] = *b"PNP0C09";

    const RD_EC: u8 = 0x80;
    const WR_EC: u8 = 0x81;
    /* const BE_EC: u8 = 0x82;
    const BD_EC: u8 = 0x83; */
    const QR_EC: u8 = 0x84;

    const OBF: u8 = 1;
    const IBF: u8 = 1 << 1;
    const SCI_EVT: u8 = 1 << 5;

    fn wait_input_buffer(&self) {
        while (read_io_byte(self.ec_sc) & Self::IBF) != 0 {
            core::hint::spin_loop()
        }
    }

    fn wait_output_buffer(&self) {
        while (read_io_byte(self.ec_sc) & Self::OBF) == 0 {
            core::hint::spin_loop()
        }
    }

    pub fn setup(interpreter: &AmlInterpreter, device_manager: &mut AcpiDeviceManager) {
        let mut ec_device_interpreter =
            if let Ok(Some(i)) = interpreter.move_into_device(&Self::HID) {
                i
            } else {
                return;
            };
        let ec_device_scope = ec_device_interpreter.get_current_scope().clone();
        pr_info!("ACPI Embedded Controller: {}", ec_device_scope);
        device_manager.ec = Some(
            match ec_device_interpreter.evaluate_method(
                &NameString::from_array(&[*b"_CRS"], false).get_full_name_path(&ec_device_scope),
                &[],
            ) {
                Ok(Some(AmlVariable::Buffer(v))) => {
                    if v.len() < 8 * 2 {
                        pr_err!("Invalid Resource Descriptors(Size: {})", v.len());
                        return;
                    }
                    if v[0] != 0x47 {
                        pr_err!("Invalid Resource Descriptors.");
                        return;
                    }
                    let ec_data = v[2] as usize;
                    if v[8] != 0x47 {
                        pr_err!("Invalid Resource Descriptors.");
                        return;
                    }
                    let ec_sc = v[10] as usize;
                    pr_info!("ACPI EC: EC_SC: {:#X}, EC_DATA: {:#X}", ec_sc, ec_data);
                    Self {
                        ec_sc,
                        ec_data,
                        gpe: None,
                        write_lock: SpinLockFlag::new(),
                    }
                }
                Ok(Some(d)) => {
                    pr_err!("Unknown Data Type: {:?}", d);
                    return;
                }
                Ok(None) => {
                    pr_err!("Invalid Method.");
                    return;
                }
                Err(_) => return,
            },
        );
        if let Ok(Some(result)) = ec_device_interpreter.evaluate_method(
            &NameString::from_array(&[*b"_STA"], false).get_full_name_path(&ec_device_scope),
            &[],
        ) {
            match result.to_int() {
                Ok(t) => {
                    if t == 0 {
                        pr_err!("Embedded Controller is disabled.");
                        device_manager.ec = None;
                        return;
                    }
                }
                Err(e) => {
                    pr_err!("Embedded Controller is disabled(_STA: {:?}).", e);
                    device_manager.ec = None;
                    return;
                }
            }
        } else {
            pr_err!("Embedded Controller is disabled.");
            device_manager.ec = None;
            return;
        }

        let arg = [
            AmlVariable::ConstData(ConstData::Byte(3)),
            AmlVariable::ConstData(ConstData::Byte(1)),
        ];
        if ec_device_interpreter
            .evaluate_method(
                &NameString::from_array(&[*b"_REG"], false).get_full_name_path(&ec_device_scope),
                &arg,
            )
            .is_err()
        {
            pr_err!("Failed to evaluate _REG.");
            device_manager.ec = None;
            return;
        }

        match ec_device_interpreter.evaluate_method(
            &NameString::from_array(&[*b"_GPE"], false).get_full_name_path(&ec_device_scope),
            &[],
        ) {
            Ok(Some(v)) => match v.to_int() {
                Ok(e) => {
                    pr_info!("GPE: {:#X}", e);
                    device_manager.ec.as_mut().unwrap().gpe = Some(e)
                }
                Err(err) => {
                    pr_warn!("Invalid GPE number: {:?}", err);
                }
            },
            Ok(None) => {
                pr_info!("No GPE");
            }
            Err(_) => {
                pr_warn!("Evaluating _GPE was failed.");
            }
        }
        let ec = device_manager.ec.as_ref().unwrap();
        while ec.is_sci_pending() {
            pr_info!("EC Query: {:#X}", ec.read_query());
        }
    }

    pub const fn get_gpe_number(&self) -> Option<usize> {
        self.gpe
    }

    pub fn read_data(&self, address: u8) -> u8 {
        let _lock = self.write_lock.lock();
        /* write_io_byte(self.ec_sc, Self::BE_EC); */
        self.wait_input_buffer();

        write_io_byte(self.ec_sc, Self::RD_EC);
        self.wait_input_buffer();

        write_io_byte(self.ec_data, address);

        self.wait_output_buffer();
        let result = read_io_byte(self.ec_data);

        /* write_io_byte(self.ec_sc, Self::BD_EC); */

        return result;
    }

    pub fn write_data(&self, address: u8, data: u8) {
        let _lock = self.write_lock.lock();
        /* write_io_byte(self.ec_sc, Self::BE_EC); */
        self.wait_input_buffer();

        write_io_byte(self.ec_sc, Self::WR_EC);
        self.wait_input_buffer();

        write_io_byte(self.ec_data, address);
        self.wait_input_buffer();

        write_io_byte(self.ec_data, data);
        self.wait_input_buffer();

        /* write_io_byte(self.ec_sc, Self::BD_EC); */

        return;
    }

    pub fn read_query(&self) -> u8 {
        let _lock = self.write_lock.lock();
        /* write_io_byte(self.ec_sc, Self::BE_EC); */
        self.wait_input_buffer();

        write_io_byte(self.ec_sc, Self::QR_EC);
        self.wait_input_buffer();

        self.wait_output_buffer();
        let result = read_io_byte(self.ec_data);

        /* write_io_byte(self.ec_sc, Self::BD_EC); */

        return result;
    }

    pub fn is_sci_pending(&self) -> bool {
        (read_io_byte(self.ec_sc) & (Self::SCI_EVT)) != 0
    }
}
