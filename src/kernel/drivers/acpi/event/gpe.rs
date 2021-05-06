///
/// General Purpose Event
///
use crate::arch::target_arch::device::acpi::write_io_byte;

pub struct GpeManager {}

impl GpeManager {
    pub fn init(gpe_block: usize, gpe_count: usize) {
        /* Clear GPE Status Bits */
        for port in gpe_block..(gpe_block + gpe_count) {
            write_io_byte(port, 0xFF);
        }

        /* Clear GPE Enable Bits */
        for port in (gpe_block + gpe_count)..(gpe_block + (gpe_count << 1)) {
            write_io_byte(port, 0x00);
        }
    }
}
