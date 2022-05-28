//!
//! DHCP
//!

use super::{ipv4, udp, AddressPrinter};
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{Address, MSize};
use crate::kfree;

const DHCP_PAYLOAD_SIZE: usize = 300;
const DHCP_SENDER_PORT: u16 = 68;
const DHCP_DESTINATION_PORT: u16 = 67;

pub fn send_dhcp_discover_packet(device_id: usize) {
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

pub fn send_dhcp_request_packet(device_id: usize, transaction_id: u32, offered_address: u32) {
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
    buffer[(DHCP_PAYLOAD_BASE + 4)..(DHCP_PAYLOAD_BASE + 8)]
        .copy_from_slice(&(transaction_id.to_be_bytes()));
    buffer[(DHCP_PAYLOAD_BASE + 16)..(DHCP_PAYLOAD_BASE + 20)]
        .copy_from_slice(&(offered_address.to_be_bytes()));
    buffer[(DHCP_PAYLOAD_BASE + 30)..(DHCP_PAYLOAD_BASE + 36)].copy_from_slice(&mac_address);
    buffer[(DHCP_PAYLOAD_BASE + DHCP_PAYLOAD_SIZE - 64)
        ..(DHCP_PAYLOAD_BASE + DHCP_PAYLOAD_SIZE - 60)]
        .copy_from_slice(&[0x63, 0x82, 0x53, 0x63]);
    buffer[(DHCP_PAYLOAD_BASE + DHCP_PAYLOAD_SIZE - 60)
        ..(DHCP_PAYLOAD_BASE + DHCP_PAYLOAD_SIZE - 57)]
        .copy_from_slice(&[0x35, 0x01, 0x03]);
    buffer[(DHCP_PAYLOAD_BASE + DHCP_PAYLOAD_SIZE - 57)
        ..(DHCP_PAYLOAD_BASE + DHCP_PAYLOAD_SIZE - 54)]
        .copy_from_slice(&[0x3d, 0x07, 0x01]);
    buffer[(DHCP_PAYLOAD_BASE + DHCP_PAYLOAD_SIZE - 54)
        ..(DHCP_PAYLOAD_BASE + DHCP_PAYLOAD_SIZE - 48)]
        .copy_from_slice(&mac_address);
    buffer[DHCP_PAYLOAD_BASE + DHCP_PAYLOAD_SIZE - 48] = 0x32;
    buffer[DHCP_PAYLOAD_BASE + DHCP_PAYLOAD_SIZE - 47] = 0x04;
    buffer[(DHCP_PAYLOAD_BASE + DHCP_PAYLOAD_SIZE - 46)
        ..(DHCP_PAYLOAD_BASE + DHCP_PAYLOAD_SIZE - 42)]
        .copy_from_slice(&offered_address.to_be_bytes());
    buffer[DHCP_PAYLOAD_BASE + DHCP_PAYLOAD_SIZE - 42] = 0xFF;

    let udp_ipv4_header = udp::create_ipv4_udp_header(
        &buffer[DHCP_PAYLOAD_BASE..],
        DHCP_SENDER_PORT,
        0,
        DHCP_DESTINATION_PORT,
        0xffffffff,
        2,
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

pub fn get_ipv4_address(device_id: usize) {
    if udp::add_udp_port_listener(udp::UdpPortListenEntry::new(
        packet_handler,
        0,
        0,
        DHCP_SENDER_PORT,
    ))
    .is_err()
    {
        pr_err!("Failed to add listener");
        return;
    }
    unsafe { DEVICE_ID = device_id };
    send_dhcp_discover_packet(device_id);
}

static mut DEVICE_ID: usize = 0;
fn packet_handler(entry: &udp::UdpPortListenEntry) {
    let dhcp_base = (entry.allocated_data_address + entry.offset).to_usize();
    if entry.data_length < entry.offset + MSize::new(240)
        || unsafe { *(dhcp_base as *const u8) } != 0x02
    {
        pr_err!("Invalid packet");
        let _ = kfree!(entry.allocated_data_address, entry.data_length);
        return;
    }

    let signature = unsafe { &*((dhcp_base + (DHCP_PAYLOAD_SIZE - 64)) as *const [u8; 4]) };
    if signature != &[0x63, 0x82, 0x53, 0x63] {
        pr_err!("Invalid packet");
        let _ = kfree!(entry.allocated_data_address, entry.data_length);
        return;
    }
    let packet_type = unsafe { &*((dhcp_base + (DHCP_PAYLOAD_SIZE - 60)) as *const [u8; 3]) };
    let transaction_id = u32::from_be_bytes(unsafe { *((dhcp_base + 4) as *const [u8; 4]) });
    pr_debug!("Transaction ID: {transaction_id}");
    let offered_address = u32::from_be_bytes(unsafe { *((dhcp_base + 16) as *const [u8; 4]) });

    if packet_type == &[0x35, 0x01, 0x02] {
        pr_debug!(
            "Offered IPv4 Address: {}",
            AddressPrinter {
                address: &offered_address.to_be_bytes(),
                separator: '.',
                is_hex: false
            },
        );
        send_dhcp_request_packet(unsafe { DEVICE_ID }, transaction_id, offered_address);
    } else if packet_type == &[0x35, 0x01, 0x05] {
        pr_debug!(
            "Request is accepted: My IPv4 Address is {}",
            AddressPrinter {
                address: &offered_address.to_be_bytes(),
                separator: '.',
                is_hex: false
            },
        );
    } else if packet_type == &[0x35, 0x01, 0x06] {
        pr_debug!(
            "Request is not accepted: Offered IPv4 Address: {}",
            AddressPrinter {
                address: &offered_address.to_be_bytes(),
                separator: '.',
                is_hex: false
            },
        );
    } else {
        pr_err!("Unknown packet type: {:#X?}", packet_type);
    }

    let _ = kfree!(entry.allocated_data_address, entry.data_length);
}
