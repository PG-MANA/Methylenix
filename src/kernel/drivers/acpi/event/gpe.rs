///
/// General Purpose Event
///
use crate::arch::target_arch::device::acpi::{read_io_byte, write_io_byte};

pub struct GpeManager {
    gpe_block: usize,
    gpe_count: usize,
}

impl GpeManager {
    pub const fn new(gpe_block: usize, gpe_count: usize) -> Self {
        Self {
            gpe_count,
            gpe_block,
        }
    }

    pub fn init(&self) {
        /* Clear GPE Status Bits */
        for port in self.gpe_block..(self.gpe_block + self.gpe_count) {
            write_io_byte(port, 0xFF);
        }

        /* Clear GPE Enable Bits */
        for port in (self.gpe_block + self.gpe_count)..(self.gpe_block + (self.gpe_count << 1)) {
            write_io_byte(port, 0x00);
        }
    }

    pub fn find_general_purpose_event(&self) -> Option<usize> {
        let mut bit = 0;
        for port in self.gpe_block..(self.gpe_block + self.gpe_count) {
            let mut status = read_io_byte(port);
            pr_info!("{:#X}: 0b{:b}", port, status);
            if status != 0 {
                /* Temporary:clear status bit */
                write_io_byte(port, status);
                while (status & 1) == 0 {
                    bit += 1;
                    status >>= 1;
                }
                return Some(bit);
            }
            bit += 8;
        }
        return None;
    }
}
