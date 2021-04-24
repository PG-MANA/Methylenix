//!
//! ACPI Embedded Controller Driver
//!

use crate::arch::target_arch::device::acpi::{read_io_byte, write_io_byte};

pub struct EmbeddedControllerManager {
    ec_sc: usize,
    ec_data: usize,
}

impl EmbeddedControllerManager {
    const RD_EC: u8 = 0x80;
    const WR_EC: u8 = 0x81;
    const BE_EC: u8 = 0x82;
    const BD_EC: u8 = 0x83;
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

    pub fn setup() -> Self {
        /* Temporary */
        Self {
            ec_sc: 0x66,
            ec_data: 0x62,
        }
    }

    pub fn read_data(&self, address: u8) -> u8 {
        write_io_byte(self.ec_sc, Self::BE_EC);
        self.wait_input_buffer();

        write_io_byte(self.ec_sc, Self::RD_EC);
        self.wait_input_buffer();

        write_io_byte(self.ec_data, address);

        self.wait_output_buffer();
        let result = read_io_byte(self.ec_data);

        write_io_byte(self.ec_sc, Self::BD_EC);

        return result;
    }

    pub fn write_data(&self, address: u8, data: u8) {
        write_io_byte(self.ec_sc, Self::BE_EC);
        self.wait_input_buffer();

        write_io_byte(self.ec_sc, Self::WR_EC);
        self.wait_input_buffer();

        write_io_byte(self.ec_data, address);
        self.wait_input_buffer();

        write_io_byte(self.ec_data, data);
        self.wait_input_buffer();

        write_io_byte(self.ec_sc, Self::BD_EC);

        return;
    }

    pub fn read_query(&self) -> u8 {
        write_io_byte(self.ec_sc, Self::BE_EC);
        self.wait_input_buffer();

        write_io_byte(self.ec_sc, Self::QR_EC);
        self.wait_output_buffer();

        let result = read_io_byte(self.ec_data);

        write_io_byte(self.ec_sc, Self::BD_EC);

        return result;
    }

    pub fn is_sci_pending(&self) -> bool {
        (read_io_byte(self.ec_sc) & (Self::SCI_EVT)) != 0
    }
}
