//!
//! DHCP
//!

use super::{
    ethernet_device::{EthernetFrameInfo, MacAddress},
    ipv4::{set_default_ipv4_address, Ipv4ConnectionInfo},
    udp::UdpConnectionInfo,
    AddressPrinter, InternetType, LinkType, NetworkError, TransportType,
};

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{MSize, VAddress};

use core::mem::MaybeUninit;

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

const DHCP_PACKET_SIZE: usize = 300;

fn write_byte_into_buffer(buffer: &mut [u8], base: usize, offset: usize, data: u8) {
    buffer[base + offset] = data;
}

fn write_bytes_into_buffer(buffer: &mut [u8], base: usize, offset: usize, data: &[u8]) {
    buffer[offset + base..(offset + base + data.len())].copy_from_slice(data);
}

fn read_bytes_from_slice<const LEN: usize>(buffer: &[u8], offset: usize) -> &[u8; LEN] {
    unsafe { &*(buffer[offset..].as_ptr() as usize as *const [u8; LEN]) }
}

pub fn create_dhcp_discover_packet(
    mac_address: &MacAddress,
    transaction_id: u32,
) -> [u8; DHCP_CLIENT_IDENTIFIER_ETHERNET_MAC_ADDRESS_OFFSET + 6 + 1] {
    let mut buffer: [u8; DHCP_CLIENT_IDENTIFIER_ETHERNET_MAC_ADDRESS_OFFSET + 6 + 1] =
        [0; DHCP_CLIENT_IDENTIFIER_ETHERNET_MAC_ADDRESS_OFFSET + 6 + 1];
    const DHCP_PAYLOAD_BASE: usize = 0;

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
        DHCP_CLIENT_MAC_ADDRESS_OFFSET,
        mac_address.inner(),
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
        mac_address.inner(),
    );
    write_byte_into_buffer(
        &mut buffer,
        DHCP_PAYLOAD_BASE,
        DHCP_CLIENT_IDENTIFIER_ETHERNET_MAC_ADDRESS_OFFSET + mac_address.inner().len(),
        DHCP_TERMINATE,
    );
    return buffer;
}

pub fn create_dhcp_request_packet(
    mac_address: &MacAddress,
    transaction_id: u32,
    offered_address: u32,
) -> [u8; DHCP_REQUEST_IP_OFFSET + 4 + 1] {
    let mut buffer: [u8; DHCP_REQUEST_IP_OFFSET + 4 + 1] = [0; DHCP_REQUEST_IP_OFFSET + 4 + 1];
    const DHCP_PAYLOAD_BASE: usize = 0;

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
        mac_address.inner(),
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
        mac_address.inner(),
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

    return buffer;
}

pub fn get_ipv4_address_sync(device_id: usize) -> Result<u32, ()> {
    let mac_address = get_kernel_manager_cluster()
        .network_manager
        .get_ethernet_mac_address(device_id);
    if let Err(e) = mac_address {
        pr_err!("Failed to get mac address: {:?}", e);
        return Err(());
    }
    let mac_address = mac_address.unwrap();
    let transaction_id = 124u32;

    let socket = get_kernel_manager_cluster()
        .network_manager
        .get_socket_manager()
        .create_socket(
            LinkType::Ethernet(EthernetFrameInfo::new(
                device_id,
                MacAddress::new(DHCP_DESTINATION_MAC_ADDRESS),
            )),
            InternetType::Ipv4(Ipv4ConnectionInfo::new(0, DHCP_DESTINATION_IPV4_ADDRESS)),
            TransportType::Udp(UdpConnectionInfo::new(
                DHCP_SENDER_PORT,
                DHCP_DESTINATION_PORT,
            )),
        )
        .and_then(|socket| {
            get_kernel_manager_cluster()
                .network_manager
                .get_socket_manager()
                .add_socket(socket)
        });
    if let Err(e) = socket {
        pr_err!("Failed to add socket: {:?}", e);
        return Err(());
    }
    let socket = socket.unwrap();

    let buffer = create_dhcp_discover_packet(&mac_address, transaction_id);

    let result = get_kernel_manager_cluster()
        .network_manager
        .get_socket_manager()
        .send_socket(
            socket,
            VAddress::new(&buffer as *const _ as usize),
            MSize::new(buffer.len()),
        )
        .and_then(|sent| {
            if sent.to_usize() == buffer.len() {
                Ok(())
            } else {
                Err(NetworkError::DataSizeError)
            }
        });

    if let Err(e) = result {
        let _ = get_kernel_manager_cluster()
            .network_manager
            .get_socket_manager()
            .close_socket(socket);
        pr_err!("Failed to send discover request: {:?}", e);
        return Err(());
    }

    let buffer: MaybeUninit<[u8; DHCP_PACKET_SIZE]> = MaybeUninit::uninit();

    let result = get_kernel_manager_cluster()
        .network_manager
        .get_socket_manager()
        .read_socket(
            socket,
            VAddress::new(buffer.as_ptr() as usize),
            MSize::new(DHCP_PACKET_SIZE),
            true,
        );

    if let Err(e) = result {
        let _ = get_kernel_manager_cluster()
            .network_manager
            .get_socket_manager()
            .close_socket(socket);
        pr_err!("Failed to read socket: {:?}", e);
        return Err(());
    }

    let read_size = result.unwrap();
    if read_size < MSize::new(DHCP_MAGIC_OFFSET + DHCP_MAGIC.len()) {
        pr_err!("Invalid packet size");
        return Err(());
    }

    let buffer = unsafe { buffer.assume_init() };
    let packet_type: &[u8; DHCP_MESSAGE_TYPE_LEN] =
        read_bytes_from_slice(&buffer, DHCP_MESSAGE_TYPE_OFFSET);
    let received_transaction_id =
        u32::from_be_bytes(*read_bytes_from_slice(&buffer, DHCP_XID_OFFSET));
    let offered_address = u32::from_be_bytes(*read_bytes_from_slice(
        &buffer,
        DHCP_OFFERED_IP_ADDRESS_OFFSET,
    ));

    if *packet_type != DHCP_MESSAGE_TYPE_OFFER || received_transaction_id != transaction_id {
        pr_err!("Invalid packet type: {:#X?}", packet_type);
        pr_err!("TransactionId: {transaction_id} <=> {received_transaction_id}");
        return Err(());
    }
    pr_debug!(
        "Offered IPv4 Address: {}",
        AddressPrinter {
            address: &offered_address.to_be_bytes(),
            separator: '.',
            is_hex: false
        },
    );

    /* Send Request */
    let buffer = create_dhcp_request_packet(&mac_address, transaction_id, offered_address);

    let result = get_kernel_manager_cluster()
        .network_manager
        .get_socket_manager()
        .send_socket(
            socket,
            VAddress::new(&buffer as *const _ as usize),
            MSize::new(buffer.len()),
        )
        .and_then(|sent| {
            if sent.to_usize() == buffer.len() {
                Ok(())
            } else {
                Err(NetworkError::DataSizeError)
            }
        });

    if let Err(e) = result {
        let _ = get_kernel_manager_cluster()
            .network_manager
            .get_socket_manager()
            .close_socket(socket);
        pr_err!("Failed to send request request: {:?}", e);
        return Err(());
    }

    let buffer: MaybeUninit<[u8; DHCP_PACKET_SIZE]> = MaybeUninit::uninit();

    let result = get_kernel_manager_cluster()
        .network_manager
        .get_socket_manager()
        .read_socket(
            socket,
            VAddress::new(buffer.as_ptr() as usize),
            MSize::new(DHCP_PACKET_SIZE),
            true,
        );

    if let Err(e) = result {
        let _ = get_kernel_manager_cluster()
            .network_manager
            .get_socket_manager()
            .close_socket(socket);
        pr_err!("Failed to read socket: {:?}", e);
        return Err(());
    }

    let _ = get_kernel_manager_cluster()
        .network_manager
        .get_socket_manager()
        .close_socket(socket);

    let read_size = result.unwrap();
    if read_size < MSize::new(DHCP_MAGIC_OFFSET + DHCP_MAGIC.len()) {
        pr_err!("Invalid packet size");
        return Err(());
    }

    let buffer = unsafe { buffer.assume_init() };
    let packet_type: &[u8; DHCP_MESSAGE_TYPE_LEN] =
        read_bytes_from_slice(&buffer, DHCP_MESSAGE_TYPE_OFFSET);
    let received_transaction_id =
        u32::from_be_bytes(*read_bytes_from_slice(&buffer, DHCP_XID_OFFSET));
    let offered_address = u32::from_be_bytes(*read_bytes_from_slice(
        &buffer,
        DHCP_OFFERED_IP_ADDRESS_OFFSET,
    ));

    if received_transaction_id != transaction_id {
        pr_err!("TransactionId: {transaction_id} <=> {received_transaction_id}");
        return Err(());
    }

    match packet_type {
        &DHCP_MESSAGE_TYPE_PACK => {
            pr_debug!(
                "Request is accepted: My IPv4 Address is {}",
                AddressPrinter {
                    address: &offered_address.to_be_bytes(),
                    separator: '.',
                    is_hex: false
                },
            );
            set_default_ipv4_address(device_id, offered_address);
            Ok(offered_address)
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
            Err(())
        }
        _ => {
            pr_err!("Unknown packet type: {:#X?}", packet_type);
            Err(())
        }
    }
}
