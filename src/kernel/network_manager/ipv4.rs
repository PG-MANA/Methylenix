//!
//! IPv4
//!

use super::{tcp, udp, LinkType, NetworkError};

use crate::kernel::memory_manager::data_type::{Address, MSize, VAddress};

use crate::kfree;

use alloc::vec::Vec;

const IPV4_VERSION: u8 = 0x04;
const IPV4_DEFAULT_IHL: u8 = 0x05;
pub const IPV4_DEFAULT_HEADER_SIZE: usize = IPV4_DEFAULT_IHL as usize * 4;
pub const ETHERNET_TYPE_IPV4: u16 = 0x0800;
const MAX_PACKET_SIZE: u16 = u16::MAX;
pub const IPV4_ADDRESS_ANY: u32 = 0;

#[repr(C)]
struct DefaultIpv4Packet {
    version_and_length: u8,
    type_of_service: u8,
    length: u16,
    id: u16,
    fragment_offset: u16,
    ttl: u8,
    protocol: u8,
    checksum: u16,
    sender_ip_address: u32,
    destination_ip_address: u32,
}

/// Ipv4ConnectionInfo is **the arrived segment information**.
/// the **sender** means **opposite**, and the **destination** means **our side**.
/// Be careful when reply data.
#[derive(Clone)]
pub struct Ipv4ConnectionInfo {
    sender_address: u32,
    destination_address: u32,
}

impl Ipv4ConnectionInfo {
    pub fn new(our_address: u32, their_address: u32) -> Self {
        Self {
            sender_address: their_address,
            destination_address: our_address,
        }
    }

    pub fn get_sender_address(&self) -> u32 {
        self.sender_address
    }

    pub fn get_destination_address(&self) -> u32 {
        self.destination_address
    }

    pub fn get_their_address(&self) -> u32 {
        self.get_sender_address()
    }

    pub fn get_our_address(&self) -> u32 {
        self.get_destination_address()
    }
}

static mut DEFAULT_IPV4_ADDRESS: Vec<u32> = Vec::new();

#[allow(dead_code)]
impl DefaultIpv4Packet {
    pub fn from_buffer(buffer: &mut [u8]) -> &mut Self {
        assert!(buffer.len() >= IPV4_DEFAULT_HEADER_SIZE);
        unsafe { &mut *(buffer.as_mut_ptr() as usize as *mut Self) }
    }

    #[cfg(target_endian = "big")]
    pub fn set_version_and_header_length(&mut self) {
        self.version_and_length = (IPV4_DEFAULT_IHL << 4) | IPV4_VERSION;
    }

    #[cfg(target_endian = "little")]
    pub fn set_version_and_header_length(&mut self) {
        self.version_and_length = (IPV4_VERSION << 4) | IPV4_DEFAULT_IHL;
    }

    #[cfg(target_endian = "big")]
    pub const fn get_version(&self) -> u8 {
        self.version_and_length & 0xf
    }

    #[cfg(target_endian = "little")]
    pub const fn get_version(&self) -> u8 {
        self.version_and_length >> 4
    }

    #[cfg(target_endian = "big")]
    pub const fn get_header_length(&self) -> usize {
        (self.version_and_length >> 4) as usize * 4
    }

    #[cfg(target_endian = "little")]
    pub const fn get_header_length(&self) -> usize {
        (self.version_and_length & 0xf) as usize * 4
    }

    pub const fn get_type_of_service(&self) -> u8 {
        self.type_of_service
    }

    pub fn set_type_of_service(&mut self, tos: u8) {
        self.type_of_service = tos;
    }

    pub const fn get_packet_length(&self) -> u16 {
        u16::from_be(self.length)
    }

    pub fn set_packet_length(&mut self, length: u16) {
        self.length = length.to_be();
    }

    pub const fn get_id(&self) -> u16 {
        u16::from_be(self.id)
    }

    pub fn set_id(&mut self, id: u16) {
        self.id = id.to_be();
    }

    pub const fn is_more_packet_flag_on(&self) -> bool {
        (u16::from_be(self.fragment_offset) & 0x2000) != 0
    }

    pub fn clear_flag_and_fragment_offset(&mut self) {
        self.fragment_offset = 0;
    }

    pub const fn get_fragment_offset(&self) -> u16 {
        (u16::from_be(self.fragment_offset) & 0x1fff) << 3
    }

    #[allow(dead_code)]
    pub fn set_fragment_offset(&mut self, fragment_offset: u16) {
        assert_eq!(fragment_offset & 0b111, 0);
        assert!((fragment_offset >> 3) <= 0x1fff);
        self.fragment_offset = ((u16::from_be(self.fragment_offset) & !0x1fff)
            | (fragment_offset >> 3) as u16)
            .to_be();
    }

    pub const fn get_ttl(&self) -> u8 {
        self.ttl
    }

    pub fn set_ttl(&mut self, ttl: u8) {
        self.ttl = ttl;
    }

    pub const fn get_protocol(&self) -> u8 {
        self.protocol
    }

    pub fn set_protocol(&mut self, protocol: u8) {
        self.protocol = protocol;
    }

    pub const fn get_checksum(&self) -> u16 {
        u16::from_be(self.checksum)
    }

    pub fn set_checksum(&mut self) {
        self.checksum = 0;
        let mut checksum: u16 = 0;
        let header_buffer =
            unsafe { &*(self as *const _ as usize as *const [u8; IPV4_DEFAULT_HEADER_SIZE]) };
        for i in 0..(IPV4_DEFAULT_HEADER_SIZE / core::mem::size_of::<u16>()) {
            let i = checksum.overflowing_add(u16::from_be_bytes([
                header_buffer[2 * i],
                header_buffer[2 * i + 1],
            ]));
            checksum = if i.1 { i.0 + 1 } else { i.0 };
        }
        self.checksum = (!checksum).to_be();
    }

    pub const fn get_sender_ip_address(&self) -> u32 {
        u32::from_be(self.sender_ip_address)
    }

    pub fn set_sender_ip_address(&mut self, address: u32) {
        self.sender_ip_address = address.to_be();
    }

    pub const fn get_destination_ip_address(&self) -> u32 {
        u32::from_be(self.destination_ip_address)
    }

    pub fn set_destination_ip_address(&mut self, address: u32) {
        self.destination_ip_address = address.to_be();
    }
}

pub fn create_default_ipv4_header(
    header_buffer: &mut [u8],
    data_size: usize,
    id: u16,
    ttl: u8,
    protocol: u8,
    sender_ipv4_address: u32,
    destination_ipv4_address: u32,
) -> Result<(), NetworkError> {
    if data_size > ((MAX_PACKET_SIZE as usize) - IPV4_DEFAULT_HEADER_SIZE) {
        return Err(NetworkError::DataSizeError);
    } else if header_buffer.len() < IPV4_DEFAULT_HEADER_SIZE {
        return Err(NetworkError::DataSizeError);
    }
    let ipv4_packet = DefaultIpv4Packet::from_buffer(header_buffer);
    ipv4_packet.set_version_and_header_length();
    ipv4_packet.set_type_of_service(0);
    ipv4_packet.set_packet_length((data_size + IPV4_DEFAULT_HEADER_SIZE) as u16);
    ipv4_packet.set_id(id);
    ipv4_packet.clear_flag_and_fragment_offset();
    ipv4_packet.set_ttl(ttl);
    ipv4_packet.set_protocol(protocol);
    ipv4_packet.set_sender_ip_address(sender_ipv4_address);
    ipv4_packet.set_destination_ip_address(destination_ipv4_address);
    ipv4_packet.set_checksum();
    return Ok(());
}

pub fn get_default_ttl() -> u8 {
    128
}

pub fn ipv4_packet_handler(
    allocated_data_base: VAddress,
    data_length: MSize,
    packet_offset: usize,
    link_info: LinkType,
) {
    let ipv4_base = allocated_data_base.to_usize() + packet_offset;
    let ipv4_packet = DefaultIpv4Packet::from_buffer(unsafe {
        &mut *(ipv4_base as *mut [u8; IPV4_DEFAULT_HEADER_SIZE])
    });
    if data_length.to_usize() < (packet_offset + IPV4_DEFAULT_HEADER_SIZE)
        || ipv4_packet.get_version() != IPV4_VERSION
    {
        pr_err!("Invalid packet");
        let _ = kfree!(allocated_data_base, data_length);
        return;
    }
    let header_length = ipv4_packet.get_header_length();
    let packet_size = ipv4_packet.get_packet_length();
    if ((packet_size as usize) + packet_offset) > data_length.to_usize() {
        pr_err!("Invalid IP packet size: {:#X}", packet_size);
        let _ = kfree!(allocated_data_base, data_length);
        return;
    }
    if ipv4_packet.is_more_packet_flag_on() {
        pr_err!("Packet is fragmented: TODO...");
        let _ = kfree!(allocated_data_base, data_length);
        return;
    }
    let ipv4_packet_info = Ipv4ConnectionInfo {
        sender_address: ipv4_packet.get_sender_ip_address(),
        destination_address: ipv4_packet.get_destination_ip_address(),
    };

    match ipv4_packet.get_protocol() {
        udp::IPV4_PROTOCOL_UDP => udp::udp_ipv4_segment_handler(
            allocated_data_base,
            data_length,
            packet_offset + header_length,
            packet_size as usize - header_length,
            link_info,
            ipv4_packet_info,
        ),
        tcp::IPV4_PROTOCOL_TCP => tcp::tcp_ipv4_packet_handler(
            allocated_data_base,
            data_length,
            packet_offset + header_length,
            packet_size as usize - header_length,
            link_info,
            ipv4_packet_info,
        ),
        t => {
            pr_err!("Unknown Protocol Type: {:#X}", t);
            let _ = kfree!(allocated_data_base, data_length);
        }
    }
}

pub fn get_default_ipv4_address(device_id: usize) -> Option<u32> {
    unsafe { DEFAULT_IPV4_ADDRESS.get(device_id) }
        .and_then(|a| if *a == 0 { None } else { Some(*a) })
}

pub fn set_default_ipv4_address(device_id: usize, address: u32) {
    if unsafe { DEFAULT_IPV4_ADDRESS.len() } <= device_id {
        unsafe { DEFAULT_IPV4_ADDRESS.resize(device_id + 1, 0) };
    }
    unsafe { DEFAULT_IPV4_ADDRESS[device_id] = address }
}
