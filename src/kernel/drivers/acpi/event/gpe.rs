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

    pub const fn get_gpe_min_number(&self) -> usize {
        self.base_number
    }

    pub const fn get_gpe_max_number(&self) -> usize {
        self.base_number + (self.gpe_count << 3) - 1
    }

    pub fn init(&self) {
        /* Clear GPE Status Bits */
        if self.gpe_block != 0 {
            for port in self.gpe_block..(self.gpe_block + self.gpe_count) {
                write_io_byte(port, 0xFF);
            }

            /* Clear GPE Enable Bits */
            for port in (self.gpe_block + self.gpe_count)..(self.gpe_block + (self.gpe_count << 1))
            {
                write_io_byte(port, 0x00);
            }
        }
    }

    pub fn enable_gpe(&self, gpe: usize) -> bool {
        if self.gpe_block == 0
            || self.base_number + (self.gpe_count << 3) < gpe
            || gpe < self.base_number
        {
            return false;
        }
        let port_index = (gpe - self.base_number) >> 3;
        let bit_index = gpe & 0b111;
        pr_info!(
            "Enable GPE{:#X} (BasePort: {:#X}, Index: {:#X}, Bit: {:#X}), Current: {}",
            gpe,
            self.gpe_block + self.gpe_count,
            port_index,
            bit_index,
            ((read_io_byte(self.gpe_block + port_index)) >> bit_index) & 1
        );
        self.clear_status_bit(gpe);
        let mut target = read_io_byte(self.gpe_block + self.gpe_count + port_index);
        target |= 1 << bit_index;
        write_io_byte(self.gpe_block + self.gpe_count + port_index, target);
        true
    }

    pub fn clear_status_bit(&self, gpe: usize) -> bool {
        if self.gpe_block == 0
            || self.base_number + (self.gpe_count << 3) < gpe
            || gpe < self.base_number
        {
            return false;
        }
        let port_index = (gpe - self.base_number) >> 3;
        let bit_index = gpe - ((gpe >> 3) << 3);
        let current_status = ((read_io_byte(self.gpe_block + port_index)) >> bit_index) & 1;
        if current_status != 0 {
            write_io_byte(self.gpe_block + port_index, 1 << bit_index);
        }
        if (((read_io_byte(self.gpe_block + port_index)) >> bit_index) & 1) != 0 {
            pr_warn!("Failed to clear StatusBit(GPE:{:#X})", gpe);
        }
        true
    }

    pub fn find_general_purpose_event(&self, skip_gpe: Option<usize>) -> Option<usize> {
        if self.gpe_block == 0 {
            return None;
        }
        let mut bit = skip_gpe
            .map(|g| (g - self.base_number) & !0b111)
            .unwrap_or(self.base_number);
        let start = skip_gpe
            .map(|g| self.gpe_block + ((g - self.base_number) & !0b111))
            .unwrap_or(self.gpe_block);
        for port in start..(self.gpe_block + self.gpe_count) {
            let mut status = read_io_byte(port) & read_io_byte(port + self.gpe_count);
            if status != 0 {
                bit += status.trailing_zeros() as usize;
                if skip_gpe.map(|g| bit > g).unwrap_or(true) {
                    return Some(bit);
                } else {
                    let mut remaining_bits = 8 - status.trailing_zeros() as usize - 1;
                    status >>= status.trailing_zeros() + 1;
                    bit += 1;
                    while status != 0 {
                        if (status & 1) != 0 {
                            return Some(bit);
                        }
                        status >>= 1;
                        bit += 1;
                        remaining_bits -= 1;
                    }
                    bit += remaining_bits;
                }
            } else {
                bit += 8;
            }
        }
        None
    }
}
