//!
//! Address Resolution Protocol
//!

use crate::kernel::manager_cluster::get_kernel_manager_cluster;

/*
#[repr(C)]
struct ArpPacket {
    hardware_type: u16,
    protocol_type: u16,
    hardware_address_length: u8,
    protocol_address_length: u8,
    op_code: u16,
}*/

const ETHERNET_TYPE_ARP: u16 = 0x0806;

const HARDWARE_TYPE_ETHERNET: u16 = 0x0001;
const PROTOCOL_TYPE_IPV4: u16 = 0x0800;

const OPCODE_REQUEST: u16 = 0x0001;

pub fn create_ethernet_ipv4_arp_packet(
    mac_address: [u8; 6],
    sender_ipv4_address: u32,
    target_ipv4_address: u32,
) -> [u8; 28] {
    let mut buffer = [0u8; 28];
    buffer[0..2].copy_from_slice(&HARDWARE_TYPE_ETHERNET.to_be_bytes());
    buffer[2..4].copy_from_slice(&PROTOCOL_TYPE_IPV4.to_be_bytes());
    buffer[4] = core::mem::size_of_val(&mac_address) as u8;
    buffer[5] = core::mem::size_of_val(&sender_ipv4_address) as u8;
    buffer[6..8].copy_from_slice(&OPCODE_REQUEST.to_be_bytes());
    buffer[8..14].copy_from_slice(&mac_address);
    buffer[14..18].copy_from_slice(&sender_ipv4_address.to_be_bytes());
    //buffer[18..24]
    buffer[24..28].copy_from_slice(&target_ipv4_address.to_be_bytes());
    buffer
}

#[allow(dead_code)]
pub fn send_ethernet_ipv4_arp_packet(
    device_id: usize,
    sender_ipv4_address: u32,
    target_ipv4_address: u32,
) -> Result<(), ()> {
    let mac_address = match get_kernel_manager_cluster()
        .ethernet_device_manager
        .get_mac_address(device_id)
    {
        Ok(a) => a,
        Err(_) => {
            pr_err!("Device is not found");
            return Err(());
        }
    };

    get_kernel_manager_cluster()
        .ethernet_device_manager
        .send_data(
            device_id,
            create_ethernet_ipv4_arp_packet(mac_address, sender_ipv4_address, target_ipv4_address)
                .as_slice(),
            [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF],
            ETHERNET_TYPE_ARP,
        )
        .and_then(|_| Ok(()))
}
