//!
//! IPv4
//!

use super::udp;
use crate::kernel::memory_manager::data_type::{Address, MSize, VAddress};

use crate::kfree;

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

pub fn ipv4_packet_handler(
    allocated_data_base: VAddress,
    data_length: MSize,
    packet_offset: usize,
    _sender_mac_address: [u8; 6],
) {
    let ipv4_base = allocated_data_base.to_usize() + packet_offset;
    if ((unsafe { *(ipv4_base as *const u8) } >> 4) & 0xf) != IPV4_VERSION {
        pr_err!(
            "Invalid IP version: {}",
            (unsafe { *(ipv4_base as *const u8) })
        );
        let _ = kfree!(allocated_data_base, data_length);
        return;
    }
    let header_length = (unsafe { *(ipv4_base as *const u8) } & 0b1111) as usize * 4;
    let packet_size = u16::from_be(unsafe { *((ipv4_base + 2) as *const u16) });
    if ((packet_size as usize) + packet_offset) > data_length.to_usize() {
        pr_err!("Invalid IP packet size: {:#X}", packet_size);
        let _ = kfree!(allocated_data_base, data_length);
        return;
    }
    let fragments = u16::from_be(unsafe { *((ipv4_base + 6) as *const u16) });
    let is_fragmented = ((fragments >> 13) & 1) != 0;
    if is_fragmented {
        pr_err!("Packet is fragmented: TODO...");
        let _ = kfree!(allocated_data_base, data_length);
        return;
    }
    let protocol_type = unsafe { *((ipv4_base + 9) as *const u8) };
    let sender_ipv4_address = u32::from_be(unsafe { *((ipv4_base + 12) as *const u32) });
    let target_ipv4_address = u32::from_be(unsafe { *((ipv4_base + 16) as *const u32) });

    match protocol_type {
        udp::IPV4_PROTOCOL_UDP => udp::udp_ipv4_packet_handler(
            allocated_data_base,
            data_length,
            packet_offset + header_length,
            sender_ipv4_address,
            target_ipv4_address,
        ),
        t => {
            pr_err!("Unknown Protocol Type: {:#X}", t);
            let _ = kfree!(allocated_data_base, data_length);
        }
    }
}
