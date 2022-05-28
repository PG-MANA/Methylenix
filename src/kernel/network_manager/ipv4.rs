//!
//! IPv4
//!

const IPV4_VERSION: u8 = 0x04;
const IPV4_DEFAULT_IHL: u8 = 0x05;
pub const IPV4_DEFAULT_HEADER_SIZE: usize = IPV4_DEFAULT_IHL as usize * 4;
pub const ETHERNET_TYPE_IPV4: u16 = 0x0800;
const MAX_PACKET_SIZE: u16 = u16::MAX;

pub fn create_default_ipv4_header(
    header_buffer: &mut [u8],
    data_size: usize,
    id: u16,
    is_final_packet: bool,
    ttl: u8,
    protocol: u8,
    sender_ipv4_address: u32,
    receiver_ipv4_address: u32,
) -> Result<(), ()> {
    if data_size > ((MAX_PACKET_SIZE as usize) - IPV4_DEFAULT_HEADER_SIZE) {
        return Err(());
    } else if header_buffer.len() > IPV4_DEFAULT_HEADER_SIZE {
        return Err(());
    }
    header_buffer[0] = (IPV4_VERSION << 4) | IPV4_DEFAULT_IHL;
    header_buffer[1] = 0; /* ToS: Empty */
    header_buffer[2..=3]
        .copy_from_slice(&((data_size + IPV4_DEFAULT_IHL as usize * 4) as u16).to_be_bytes());
    header_buffer[4..=5].copy_from_slice(&id.to_be_bytes());
    header_buffer[6..=7]
        .copy_from_slice(&(0 << 15 | (0 << 14) | ((!is_final_packet as u16) << 13)).to_be_bytes());
    header_buffer[8] = ttl;
    header_buffer[9] = protocol;
    header_buffer[12..=15].copy_from_slice(&sender_ipv4_address.to_be_bytes());
    header_buffer[16..=19].copy_from_slice(&receiver_ipv4_address.to_be_bytes());

    header_buffer[10] = 0;
    header_buffer[11] = 0;
    let mut checksum: u16 = 0;
    for i in 0..(IPV4_DEFAULT_HEADER_SIZE / core::mem::size_of::<u16>()) {
        let i = checksum.overflowing_add(u16::from_be_bytes([
            header_buffer[2 * i],
            header_buffer[2 * i + 1],
        ]));
        checksum = if i.1 { i.0 + 1 } else { i.0 };
    }
    header_buffer[10..=11].copy_from_slice(&(!checksum).to_be_bytes());
    return Ok(());
}

pub fn get_default_ttl() -> u8 {
    128
}
