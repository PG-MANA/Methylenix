///
/// General Purpose Event
///
use crate::arch::target_arch::device::acpi::{read_io_byte, write_io_byte};

pub struct GpeManager {
    gpe_block: usize,
    gpe_count: usize,
    base_number: usize,
}

impl GpeManager {
    pub const fn new(gpe_block: usize, gpe_count: usize, base_number: usize) -> Self {
        Self {
            gpe_count,
            gpe_block,
            base_number,
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

    pub fn enable_gpe(&self, gpe: usize) -> bool {
        if self.base_number + (self.gpe_count << 3) < gpe || gpe < self.base_number {
            return false;
        }
        let port_index = (gpe - self.base_number) >> 3;
        let bit_index = gpe - ((gpe >> 3) << 3);
        pr_info!(
            "Enable GPE{:#X} (BasePort: {:#X}, Index: {:#X}, Bit: {:#X})",
            gpe,
            self.gpe_block + self.gpe_count,
            port_index,
            bit_index
        );
        self.clear_status_bit(gpe);
        let mut target = read_io_byte(self.gpe_block + self.gpe_count + port_index);
        target |= 1 << bit_index;
        write_io_byte(self.gpe_block + self.gpe_count + port_index, target);
        return true;
    }

    pub fn clear_status_bit(&self, gpe: usize) -> bool {
        if self.base_number + (self.gpe_count << 3) < gpe || gpe < self.base_number {
            return false;
        }
        let port_index = (gpe - self.base_number) >> 3;
        let bit_index = gpe - ((gpe >> 3) << 3);
        let current_status = ((read_io_byte(self.gpe_block + port_index)) >> bit_index) & 1;
        if current_status != 0 {
            write_io_byte(self.gpe_block + port_index, 1 << bit_index);
        }
        return true;
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
