//!
//! UDP
//!

use super::{ipv4, InternetType, LinkType};

use crate::kernel::memory_manager::data_type::{Address, MSize, VAddress};

use crate::kfree;

use crate::kernel::manager_cluster::get_kernel_manager_cluster;

pub const UDP_HEADER_SIZE: usize = 0x08;
pub const IPV4_PROTOCOL_UDP: u8 = 0x11;
pub const UDP_PORT_ANY: u16 = 0;

#[repr(C)]
struct UdpSegment {
    sender_port: u16,
    destination_port: u16,
    segment_length: u16,
    checksum: u16,
}

pub struct UdpConnectionInfo {
    sender_port: u16,
    destination_port: u16,
}

pub struct UdpSegmentInfo {
    pub connection_info: UdpConnectionInfo,
    pub payload_size: usize,
}

impl UdpConnectionInfo {
    pub fn new(our_port: u16, their_port: u16) -> Self {
        Self {
            sender_port: their_port,
            destination_port: our_port,
        }
    }

    pub fn get_sender_port(&self) -> u16 {
        self.sender_port
    }

    pub fn get_destination_port(&self) -> u16 {
        self.destination_port
    }
}

impl UdpSegment {
    #[allow(dead_code)]
    pub const fn new(sender_port: u16, destination_port: u16, segment_length: u16) -> Self {
        assert!(core::mem::size_of::<Self>() == UDP_HEADER_SIZE);
        Self {
            sender_port: sender_port.to_be(),
            destination_port: destination_port.to_be(),
            segment_length: segment_length.to_be(),
            checksum: 0,
        }
    }

    pub fn from_buffer(buffer: &mut [u8]) -> &mut Self {
        assert!(buffer.len() >= UDP_HEADER_SIZE);
        unsafe { &mut *(buffer.as_mut_ptr() as usize as *mut Self) }
    }

    pub const fn get_sender_port(&self) -> u16 {
        u16::from_be(self.sender_port)
    }

    pub fn set_sender_port(&mut self, port: u16) {
        self.sender_port = port.to_be();
    }

    pub const fn get_destination_port(&self) -> u16 {
        u16::from_be(self.destination_port)
    }

    pub const fn get_segment_length(&self) -> u16 {
        u16::from_be(self.segment_length)
    }

    pub fn set_segment_length(&mut self, length: u16) {
        self.segment_length = length.to_be();
    }

    pub const fn get_checksum(&self) -> u16 {
        u16::from_be(self.checksum)
    }

    #[allow(dead_code)]
    pub const fn set_checksum(&mut self, checksum: u16) {
        self.checksum = checksum.to_be();
    }
}

pub fn create_ipv4_udp_header(
    buffer: &mut [u8; UDP_HEADER_SIZE + ipv4::IPV4_DEFAULT_HEADER_SIZE],
    data: &[u8],
    sender_port: u16,
    sender_ipv4_address: u32,
    destination_port: u16,
    destination_ipv4_address: u32,
    ipv4_packet_id: u16,
) -> Result<(), ()> {
    if (data.len() + UDP_HEADER_SIZE) > u16::MAX as usize {
        return Err(());
    }
    *buffer = [0u8; UDP_HEADER_SIZE + ipv4::IPV4_DEFAULT_HEADER_SIZE];
    let (ipv4_header, udp_header) = buffer.split_at_mut(ipv4::IPV4_DEFAULT_HEADER_SIZE);
    let udp_header = UdpSegment::from_buffer(udp_header);

    udp_header.set_sender_port(sender_port);
    udp_header.set_destination_port(destination_port);
    udp_header.set_segment_length((data.len() + UDP_HEADER_SIZE) as u16);

    ipv4::create_default_ipv4_header(
        ipv4_header,
        udp_header.get_packet_length() as usize,
        ipv4_packet_id,
        ipv4::get_default_ttl(),
        IPV4_PROTOCOL_UDP,
        sender_ipv4_address,
        destination_ipv4_address,
    )?;
    return Ok(());
}

pub fn udp_ipv4_segment_handler(
    allocated_data_base: VAddress,
    data_length: MSize,
    segment_offset: usize,
    segment_size: usize,
    link_info: LinkType,
    ipv4_packet_info: ipv4::Ipv4ConnectionInfo,
) {
    let udp_base = allocated_data_base.to_usize() + segment_offset;
    if (segment_offset + UDP_HEADER_SIZE) > data_length.to_usize() {
        pr_err!("Invalid UDP segment");
        let _ = kfree!(allocated_data_base, data_length);
        return;
    }
    let udp_segment =
        UdpSegment::from_buffer(unsafe { &mut *(udp_base as *mut [u8; UDP_HEADER_SIZE]) });
    if segment_size != udp_segment.get_segment_length() as usize {
        pr_err!(
            "Invalid UDP packet size: Expected {:#X} bytes, but Actually {:#X} bytes",
            segment_size,
            udp_packet.get_segment_length()
        );
        let _ = kfree!(allocated_data_base, data_length);
        return;
    }

    let udp_segment_info = UdpSegmentInfo {
        connection_info: UdpConnectionInfo {
            sender_port: udp_segment.get_sender_port(),
            destination_port: udp_segment.get_destination_port(),
        },
        payload_size: udp_segment.get_segment_length() as usize - UDP_HEADER_SIZE,
    };

    get_kernel_manager_cluster()
        .network_manager
        .get_socket_manager()
        .udp_segment_handler(
            link_info,
            InternetType::Ipv4(ipv4_packet_info),
            udp_segment_info,
            allocated_data_base,
            data_length,
            segment_offset + UDP_HEADER_SIZE,
        )
}
