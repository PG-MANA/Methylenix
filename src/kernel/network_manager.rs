//!
//! Network Manager
//!

pub mod arp;
pub mod dhcp;
pub mod ethernet_device;
pub mod ipv4;
pub mod udp;

struct AddressPrinter<'a> {
    address: &'a [u8],
    is_hex: bool,
    separator: char,
}

impl<'a> core::fmt::Display for AddressPrinter<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use core::fmt::Write;
        for (i, d) in self.address.iter().enumerate() {
            if self.is_hex {
                f.write_fmt(format_args!("{:02X}", *d))?;
            } else {
                f.write_fmt(format_args!("{}", *d))?;
            }
            if i != self.address.len() - 1 {
                f.write_char(self.separator)?;
            }
        }
        return Ok(());
    }
}
