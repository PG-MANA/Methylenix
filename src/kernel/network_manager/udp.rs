//!
//! UDP
//!

pub const UDP_HEADER_SIZE: usize = 0x08;
pub const IPV4_PROTOCOL_UDP: u8 = 0x11;

pub fn create_udp_header(
    data: &[u8],
    must_calculate_checksum: bool,
    sender_port: u16,
    destination_port: u16,
) -> Result<[u8; UDP_HEADER_SIZE], ()> {
    if data.len() > u16::MAX as usize {
        return Err(());
    }
    let mut header = [0u8; UDP_HEADER_SIZE];
    header[0..=1].copy_from_slice(&sender_port.to_be_bytes());
    header[2..=3].copy_from_slice(&destination_port.to_be_bytes());
    header[4..=5].copy_from_slice(&(data.len() as u16).to_be_bytes());
    if must_calculate_checksum {
        unimplemented!();
    }
    return Ok(header);
}
