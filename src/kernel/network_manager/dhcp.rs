//!
//! DHCP
//!

use super::{ipv4, udp};
use crate::kernel::manager_cluster::get_kernel_manager_cluster;

const DHCP_PAYLOAD_SIZE: usize = 300;
const DHCP_SENDER_PORT: u16 = 68;
const DHCP_DESTINATION_PORT: u16 = 67;

pub fn create_dhcp_discover_packet(device_id: usize) {
    let mac_address = match get_kernel_manager_cluster()
        .ethernet_device_manager
        .get_mac_address(device_id)
    {
        Ok(a) => a,
        Err(_) => {
            pr_err!("Device is not found");
            return;
        }
    };
    let mut buffer =
        [0u8; DHCP_PAYLOAD_SIZE + udp::UDP_HEADER_SIZE + ipv4::IPV4_DEFAULT_HEADER_SIZE];
    const DHCP_PAYLOAD_BASE: usize = udp::UDP_HEADER_SIZE + ipv4::IPV4_DEFAULT_HEADER_SIZE;
    buffer[DHCP_PAYLOAD_BASE] = 0x01;
    buffer[DHCP_PAYLOAD_BASE + 1] = 0x01;
    buffer[DHCP_PAYLOAD_BASE + 2] = 0x06;
    buffer[(DHCP_PAYLOAD_BASE + 4)..(DHCP_PAYLOAD_BASE + 8)].copy_from_slice(&(1u32.to_be_bytes()));
    buffer[(DHCP_PAYLOAD_BASE + 30)..(DHCP_PAYLOAD_BASE + 36)].copy_from_slice(&mac_address);
    buffer[(DHCP_PAYLOAD_BASE + DHCP_PAYLOAD_SIZE - 64)
        ..(DHCP_PAYLOAD_BASE + DHCP_PAYLOAD_SIZE - 60)]
        .copy_from_slice(&[0x63, 0x82, 0x53, 0x63]);
    buffer[(DHCP_PAYLOAD_BASE + DHCP_PAYLOAD_SIZE - 60)
        ..(DHCP_PAYLOAD_BASE + DHCP_PAYLOAD_SIZE - 57)]
        .copy_from_slice(&[0x35, 0x01, 0x01]);
    buffer[(DHCP_PAYLOAD_BASE + DHCP_PAYLOAD_SIZE - 57)
        ..(DHCP_PAYLOAD_BASE + DHCP_PAYLOAD_SIZE - 54)]
        .copy_from_slice(&[0x3d, 0x07, 0x01]);
    buffer[(DHCP_PAYLOAD_BASE + DHCP_PAYLOAD_SIZE - 54)
        ..(DHCP_PAYLOAD_BASE + DHCP_PAYLOAD_SIZE - 48)]
        .copy_from_slice(&mac_address);
    buffer[DHCP_PAYLOAD_BASE + DHCP_PAYLOAD_SIZE - 48] = 0xFF;

    let udp_ipv4_header = udp::create_ipv4_udp_header(
        &buffer[DHCP_PAYLOAD_BASE..],
        DHCP_SENDER_PORT,
        0,
        DHCP_DESTINATION_PORT,
        0xffffffff,
        1,
    )
    .expect("Failed to create packet");
    buffer[0..DHCP_PAYLOAD_BASE].copy_from_slice(&udp_ipv4_header);

    let _ = get_kernel_manager_cluster()
        .ethernet_device_manager
        .send_data(
            device_id,
            buffer.as_slice(),
            [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF],
            ipv4::ETHERNET_TYPE_IPV4,
        );
}
