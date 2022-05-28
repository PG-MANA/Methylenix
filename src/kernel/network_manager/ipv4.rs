//!
//! IPv4
//!

const IPV4_VERSION: u8 = 0x04;
const IPV4_DEFAULT_IHL: u8 = 0x05;
pub const IPV4_DEFAULT_HEADER_SIZE: usize = IPV4_DEFAULT_IHL as usize * 4;
pub const ETHERNET_TYPE_IPV4: u16 = 0x0800;
const MAX_PACKET_SIZE: u16 = u16::MAX;

pub fn create_default_ipv4_header(
    data_size: usize,
    id: u16,
    is_final_packet: bool,
    ttl: u8,
    protocol: u8,
    sender_ipv4_address: u32,
    receiver_ipv4_address: u32,
) -> Result<[u8; IPV4_DEFAULT_HEADER_SIZE], ()> {
    if data_size > ((MAX_PACKET_SIZE as usize) - IPV4_DEFAULT_HEADER_SIZE) {
        return Err(());
    }
    let mut header: [u8; IPV4_DEFAULT_HEADER_SIZE] = [0; IPV4_DEFAULT_HEADER_SIZE];
    header[0] = (IPV4_VERSION << 4) | IPV4_DEFAULT_IHL;
    /* ToS: Empty */
    header[2..=3]
        .copy_from_slice(&((data_size + IPV4_DEFAULT_IHL as usize * 4) as u16).to_be_bytes());
    header[4..=5].copy_from_slice(&id.to_be_bytes());
    header[6..=7]
        .copy_from_slice(&(0 << 15 | (0 << 14) | ((!is_final_packet as u16) << 13)).to_be_bytes());
    header[8] = ttl;
    header[9] = protocol;
    header[12..=15].copy_from_slice(&sender_ipv4_address.to_be_bytes());
    header[16..=19].copy_from_slice(&receiver_ipv4_address.to_be_bytes());

    let mut checksum: u16 = 0;
    for i in 0..(IPV4_DEFAULT_HEADER_SIZE / core::mem::size_of::<u16>()) {
        let i = checksum.overflowing_add(u16::from_be_bytes([header[2 * i], header[2 * i + 1]]));
        checksum = if i.1 { i.0 + 1 } else { i.0 };
    }
    header[10..=11].copy_from_slice(&(!checksum).to_be_bytes());
    return Ok(header);
}
