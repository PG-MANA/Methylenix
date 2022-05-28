//!
//! UDP
//!

use super::ipv4;

pub const UDP_HEADER_SIZE: usize = 0x08;
pub const IPV4_PROTOCOL_UDP: u8 = 0x11;

pub fn create_ipv4_udp_header(
    data: &[u8],
    sender_port: u16,
    sender_ipv4_address: u32,
    destination_port: u16,
    destination_ipv4_address: u32,
    packet_id: u16,
) -> Result<[u8; UDP_HEADER_SIZE + ipv4::IPV4_DEFAULT_HEADER_SIZE], ()> {
    if data.len() > u16::MAX as usize {
        return Err(());
    }
    let mut header = [0u8; UDP_HEADER_SIZE + ipv4::IPV4_DEFAULT_HEADER_SIZE];
    const UDP_HEADER_BASE: usize = ipv4::IPV4_DEFAULT_HEADER_SIZE;

    header[UDP_HEADER_BASE..=(UDP_HEADER_BASE + 1)].copy_from_slice(&sender_port.to_be_bytes());
    header[(UDP_HEADER_BASE + 2)..=(UDP_HEADER_BASE + 3)]
        .copy_from_slice(&destination_port.to_be_bytes());
    header[(UDP_HEADER_BASE + 4)..=(UDP_HEADER_BASE + 5)]
        .copy_from_slice(&(data.len() as u16).to_be_bytes());

    ipv4::create_default_ipv4_header(
        &mut header[0..UDP_HEADER_BASE],
        data.len(),
        packet_id,
        true,
        ipv4::get_default_ttl(),
        IPV4_PROTOCOL_UDP,
        sender_ipv4_address,
        destination_ipv4_address,
    )?;

    return Ok(header);
}
