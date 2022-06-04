//!
//! DHCP
//!

use super::{ipv4, udp, AddressPrinter};

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{Address, MSize};

use crate::kfree;

const DHCP_SENDER_PORT: u16 = 68;
const DHCP_DESTINATION_PORT: u16 = 67;

const DHCP_MESSAGE_OP_OFFSET: usize = 0x00;
const DHCP_MESSAGE_OP_DISCOVER: u8 = 0x01;

const DHCP_HARDWARE_TYPE_OFFSET: usize = 0x01;
const DHCP_HARDWARE_TYPE_ETHERNET: u8 = 0x01;

const DHCP_HARDWARE_LENGTH_OFFSET: usize = 0x02;
const DHCP_HARDWARE_LENGTH_ETHERNET: u8 = 0x06;

const DHCP_XID_OFFSET: usize = 0x04;

const DHCP_OFFERED_IP_ADDRESS_OFFSET: usize = 0x10;

const DHCP_CLIENT_MAC_ADDRESS_OFFSET: usize = 0x1E;

const DHCP_MAGIC_OFFSET: usize = 0xEC;
const DHCP_MAGIC: [u8; 4] = [0x63, 0x82, 0x53, 0x63];

const DHCP_MESSAGE_TYPE_OFFSET: usize = 0xF0;
const DHCP_MESSAGE_TYPE_LEN: usize = 3;
const DHCP_MESSAGE_TYPE_DISCOVER: [u8; DHCP_MESSAGE_TYPE_LEN] = [0x35, 0x01, 0x01];
const DHCP_MESSAGE_TYPE_OFFER: [u8; DHCP_MESSAGE_TYPE_LEN] = [0x35, 0x01, 0x02];
const DHCP_MESSAGE_TYPE_REQUEST: [u8; DHCP_MESSAGE_TYPE_LEN] = [0x35, 0x01, 0x03];
const DHCP_MESSAGE_TYPE_PACK: [u8; DHCP_MESSAGE_TYPE_LEN] = [0x35, 0x01, 0x05];
const DHCP_MESSAGE_TYPE_PNACK: [u8; DHCP_MESSAGE_TYPE_LEN] = [0x35, 0x01, 0x06];

const DHCP_CLIENT_IDENTIFIER_OFFSET: usize = 0xF3;
const DHCP_CLIENT_IDENTIFIER_ETHERNET: [u8; 3] = [0x3D, 0x07, 0x01];
const DHCP_CLIENT_IDENTIFIER_ETHERNET_MAC_ADDRESS_OFFSET: usize = 0x0F6;

const DHCP_REQUEST_IP_HEAD_OFFSET: usize = 0xFC;
const DHCP_REQUEST_IP_HEAD: [u8; 2] = [0x32, 0x04];

const DHCP_REQUEST_IP_OFFSET: usize = 0xFE;

const DHCP_TERMINATE: u8 = 0xFF;

const DHCP_DESTINATION_MAC_ADDRESS: [u8; 6] = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
const DHCP_DESTINATION_IPV4_ADDRESS: u32 = 0xffff_ffff;

fn write_byte_into_buffer(buffer: &mut [u8], base: usize, offset: usize, data: u8) {
    buffer[base + offset] = data;
}

fn write_bytes_into_buffer(buffer: &mut [u8], base: usize, offset: usize, data: &[u8]) {
    buffer[offset + base..(offset + base + data.len())].copy_from_slice(data);
}

fn read_bytes_from_slice<const LEN: usize>(buffer: &[u8], offset: usize) -> &[u8; LEN] {
    unsafe { &*(buffer[offset..].as_ptr() as usize as *const [u8; LEN]) }
}

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
    let mut buffer: [u8; (DHCP_CLIENT_IDENTIFIER_ETHERNET_MAC_ADDRESS_OFFSET + 6 + 1)
        + udp::UDP_HEADER_SIZE
        + ipv4::IPV4_DEFAULT_HEADER_SIZE] = [0;
        (DHCP_CLIENT_IDENTIFIER_ETHERNET_MAC_ADDRESS_OFFSET + 6 + 1)
            + udp::UDP_HEADER_SIZE
            + ipv4::IPV4_DEFAULT_HEADER_SIZE];

    const DHCP_PAYLOAD_BASE: usize = udp::UDP_HEADER_SIZE + ipv4::IPV4_DEFAULT_HEADER_SIZE;
    write_byte_into_buffer(
        &mut buffer,
        DHCP_PAYLOAD_BASE,
        DHCP_MESSAGE_OP_OFFSET,
        DHCP_MESSAGE_OP_DISCOVER,
    );
    write_byte_into_buffer(
        &mut buffer,
        DHCP_PAYLOAD_BASE,
        DHCP_HARDWARE_TYPE_OFFSET,
        DHCP_HARDWARE_TYPE_ETHERNET,
    );
    write_byte_into_buffer(
        &mut buffer,
        DHCP_PAYLOAD_BASE,
        DHCP_HARDWARE_LENGTH_OFFSET,
        DHCP_HARDWARE_LENGTH_ETHERNET,
    );
    write_bytes_into_buffer(
        &mut buffer,
        DHCP_PAYLOAD_BASE,
        DHCP_XID_OFFSET,
        &1u32.to_be_bytes(),
    );
    write_bytes_into_buffer(
        &mut buffer,
        DHCP_PAYLOAD_BASE,
        DHCP_CLIENT_MAC_ADDRESS_OFFSET,
        &mac_address,
    );
    write_bytes_into_buffer(
        &mut buffer,
        DHCP_PAYLOAD_BASE,
        DHCP_MAGIC_OFFSET,
        &DHCP_MAGIC,
    );
    write_bytes_into_buffer(
        &mut buffer,
        DHCP_PAYLOAD_BASE,
        DHCP_MESSAGE_TYPE_OFFSET,
        &DHCP_MESSAGE_TYPE_DISCOVER,
    );
    write_bytes_into_buffer(
        &mut buffer,
        DHCP_PAYLOAD_BASE,
        DHCP_CLIENT_IDENTIFIER_OFFSET,
        &DHCP_CLIENT_IDENTIFIER_ETHERNET,
    );
    write_bytes_into_buffer(
        &mut buffer,
        DHCP_PAYLOAD_BASE,
        DHCP_CLIENT_IDENTIFIER_ETHERNET_MAC_ADDRESS_OFFSET,
        &mac_address,
    );
    write_byte_into_buffer(
        &mut buffer,
        DHCP_PAYLOAD_BASE,
        DHCP_CLIENT_IDENTIFIER_ETHERNET_MAC_ADDRESS_OFFSET + mac_address.len(),
        DHCP_TERMINATE,
    );

    let udp_ipv4_header = udp::create_ipv4_udp_header(
        &buffer[DHCP_PAYLOAD_BASE..],
        DHCP_SENDER_PORT,
        0,
        DHCP_DESTINATION_PORT,
        DHCP_DESTINATION_IPV4_ADDRESS,
        1,
    )
    .expect("Failed to create packet");
    buffer[0..DHCP_PAYLOAD_BASE].copy_from_slice(&udp_ipv4_header);

    let _ = get_kernel_manager_cluster()
        .ethernet_device_manager
        .send_data(
            device_id,
            &buffer,
            DHCP_DESTINATION_MAC_ADDRESS,
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
    let mut buffer: [u8; (DHCP_REQUEST_IP_OFFSET + 4 + 1)
        + udp::UDP_HEADER_SIZE
        + ipv4::IPV4_DEFAULT_HEADER_SIZE] = [0; (DHCP_REQUEST_IP_OFFSET + 4 + 1)
        + udp::UDP_HEADER_SIZE
        + ipv4::IPV4_DEFAULT_HEADER_SIZE];

    const DHCP_PAYLOAD_BASE: usize = udp::UDP_HEADER_SIZE + ipv4::IPV4_DEFAULT_HEADER_SIZE;
    write_byte_into_buffer(
        &mut buffer,
        DHCP_PAYLOAD_BASE,
        DHCP_MESSAGE_OP_OFFSET,
        DHCP_MESSAGE_OP_DISCOVER,
    );
    write_byte_into_buffer(
        &mut buffer,
        DHCP_PAYLOAD_BASE,
        DHCP_HARDWARE_TYPE_OFFSET,
        DHCP_HARDWARE_TYPE_ETHERNET,
    );
    write_byte_into_buffer(
        &mut buffer,
        DHCP_PAYLOAD_BASE,
        DHCP_HARDWARE_LENGTH_OFFSET,
        DHCP_HARDWARE_LENGTH_ETHERNET,
    );
    write_bytes_into_buffer(
        &mut buffer,
        DHCP_PAYLOAD_BASE,
        DHCP_XID_OFFSET,
        &transaction_id.to_be_bytes(),
    );
    write_bytes_into_buffer(
        &mut buffer,
        DHCP_PAYLOAD_BASE,
        DHCP_OFFERED_IP_ADDRESS_OFFSET,
        &offered_address.to_be_bytes(),
    );
    write_bytes_into_buffer(
        &mut buffer,
        DHCP_PAYLOAD_BASE,
        DHCP_CLIENT_MAC_ADDRESS_OFFSET,
        &mac_address,
    );
    write_bytes_into_buffer(
        &mut buffer,
        DHCP_PAYLOAD_BASE,
        DHCP_MAGIC_OFFSET,
        &DHCP_MAGIC,
    );
    write_bytes_into_buffer(
        &mut buffer,
        DHCP_PAYLOAD_BASE,
        DHCP_MESSAGE_TYPE_OFFSET,
        &DHCP_MESSAGE_TYPE_REQUEST,
    );
    write_bytes_into_buffer(
        &mut buffer,
        DHCP_PAYLOAD_BASE,
        DHCP_CLIENT_IDENTIFIER_OFFSET,
        &DHCP_CLIENT_IDENTIFIER_ETHERNET,
    );
    write_bytes_into_buffer(
        &mut buffer,
        DHCP_PAYLOAD_BASE,
        DHCP_CLIENT_IDENTIFIER_ETHERNET_MAC_ADDRESS_OFFSET,
        &mac_address,
    );
    write_byte_into_buffer(
        &mut buffer,
        DHCP_PAYLOAD_BASE,
        DHCP_CLIENT_IDENTIFIER_ETHERNET_MAC_ADDRESS_OFFSET + mac_address.len(),
        DHCP_TERMINATE,
    );
    write_bytes_into_buffer(
        &mut buffer,
        DHCP_PAYLOAD_BASE,
        DHCP_REQUEST_IP_HEAD_OFFSET,
        &DHCP_REQUEST_IP_HEAD,
    );
    write_bytes_into_buffer(
        &mut buffer,
        DHCP_PAYLOAD_BASE,
        DHCP_REQUEST_IP_OFFSET,
        &offered_address.to_be_bytes(),
    );
    write_byte_into_buffer(
        &mut buffer,
        DHCP_PAYLOAD_BASE,
        DHCP_REQUEST_IP_OFFSET + offered_address.to_be_bytes().len(),
        DHCP_TERMINATE,
    );

    let udp_ipv4_header = udp::create_ipv4_udp_header(
        &buffer[DHCP_PAYLOAD_BASE
            ..=(DHCP_PAYLOAD_BASE + DHCP_REQUEST_IP_OFFSET + offered_address.to_be_bytes().len())],
        DHCP_SENDER_PORT,
        0,
        DHCP_DESTINATION_PORT,
        DHCP_DESTINATION_IPV4_ADDRESS,
        2,
    )
    .expect("Failed to create packet");

    buffer[0..DHCP_PAYLOAD_BASE].copy_from_slice(&udp_ipv4_header);

    let _ = get_kernel_manager_cluster()
        .ethernet_device_manager
        .send_data(
            device_id,
            &buffer,
            DHCP_DESTINATION_MAC_ADDRESS,
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
    if entry.data_length < entry.offset + MSize::new(DHCP_MAGIC_OFFSET + DHCP_MAGIC.len()) {
        pr_err!("Invalid packet size");
        let _ = kfree!(entry.allocated_data_address, entry.data_length);
        return;
    }

    let dhcp_packet = unsafe {
        core::slice::from_raw_parts(
            (entry.allocated_data_address + entry.offset).to_usize() as *const u8,
            (entry.data_length - entry.offset).to_usize(),
        )
    };

    if dhcp_packet[DHCP_MAGIC_OFFSET..(DHCP_MAGIC_OFFSET + DHCP_MAGIC.len())] != DHCP_MAGIC {
        pr_err!("DHCP Signature is invalid");
        let _ = kfree!(entry.allocated_data_address, entry.data_length);
        return;
    }
    let packet_type: &[u8; DHCP_MESSAGE_TYPE_LEN] =
        read_bytes_from_slice(dhcp_packet, DHCP_MESSAGE_TYPE_OFFSET);
    let transaction_id = u32::from_be_bytes(*read_bytes_from_slice(dhcp_packet, DHCP_XID_OFFSET));
    let offered_address = u32::from_be_bytes(*read_bytes_from_slice(
        dhcp_packet,
        DHCP_OFFERED_IP_ADDRESS_OFFSET,
    ));

    pr_debug!("Transaction ID: {transaction_id}");
    match packet_type {
        &DHCP_MESSAGE_TYPE_OFFER => {
            pr_debug!(
                "Offered IPv4 Address: {}",
                AddressPrinter {
                    address: &offered_address.to_be_bytes(),
                    separator: '.',
                    is_hex: false
                },
            );
            send_dhcp_request_packet(unsafe { DEVICE_ID }, transaction_id, offered_address);
        }
        &DHCP_MESSAGE_TYPE_PACK => {
            pr_debug!(
                "Request is accepted: My IPv4 Address is {}",
                AddressPrinter {
                    address: &offered_address.to_be_bytes(),
                    separator: '.',
                    is_hex: false
                },
            );
            ipv4::set_default_ipv4_address(unsafe { DEVICE_ID }, offered_address);
        }
        &DHCP_MESSAGE_TYPE_PNACK => {
            pr_debug!(
                "Request is not accepted: Offered IPv4 Address: {}",
                AddressPrinter {
                    address: &offered_address.to_be_bytes(),
                    separator: '.',
                    is_hex: false
                },
            );
            send_dhcp_discover_packet(unsafe { DEVICE_ID });
        }
        _ => {
            pr_err!("Unknown packet type: {:#X?}", packet_type);
        }
    }

    let _ = kfree!(entry.allocated_data_address, entry.data_length);
}
