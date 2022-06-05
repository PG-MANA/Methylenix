//!
//! Network Manager
//!

use crate::kernel::memory_manager::data_type::{MSize, VAddress};

pub mod arp;
pub mod dhcp;
pub mod ethernet_device;
pub mod ipv4;
pub mod socket_manager;
pub mod tcp;
pub mod udp;

struct AddressPrinter<'a> {
    address: &'a [u8],
    is_hex: bool,
    separator: char,
}

impl<'a> core::fmt::Display for AddressPrinter<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use core::fmt::Write;
        for (i, d) in self.address.iter().enumerate() {
            if self.is_hex {
                f.write_fmt(format_args!("{:02X}", *d))?;
            } else {
                f.write_fmt(format_args!("{}", *d))?;
            }
            if i != self.address.len() - 1 {
                f.write_char(self.separator)?;
            }
        }
        return Ok(());
    }
}

#[derive(Clone, Eq, PartialEq)]
pub enum AddressInfo {
    Any,
    Ipv4(u32),
    Ipv6([u8; 16]),
}

pub struct NetworkManager {
    ethernet_manager: ethernet_device::EthernetDeviceManager,
    socket_manager: socket_manager::SocketManager,
}

impl NetworkManager {
    pub fn init(&mut self) {
        core::mem::forget(core::mem::replace(
            &mut self.ethernet_manager,
            ethernet_device::EthernetDeviceManager::new(),
        ));
        core::mem::forget(core::mem::replace(
            &mut self.socket_manager,
            socket_manager::SocketManager::new(),
        ));
    }

    pub fn add_ethernet_device(
        &mut self,
        descriptor: ethernet_device::EthernetDeviceDescriptor,
    ) -> usize {
        self.ethernet_manager.add_device(descriptor)
    }

    pub fn received_ethernet_frame_handler(
        &mut self,
        device_id: usize,
        allocated_data: VAddress,
        length: MSize,
    ) {
        self.ethernet_manager
            .received_data_handler(device_id, allocated_data, length)
    }

    pub fn update_ethernet_transmit_status(
        &mut self,
        device_id: usize,
        id: u32,
        is_successful: bool,
    ) {
        self.ethernet_manager
            .update_transmit_status(device_id, id, is_successful)
    }

    pub fn get_socket_manager(&mut self) -> &mut socket_manager::SocketManager {
        &mut self.socket_manager
    }

    pub fn get_ethernet_mac_address(&self, device_id: usize) -> Result<[u8; 6], ()> {
        self.ethernet_manager.get_mac_address(device_id)
    }

    /// Arguments will be changed.
    pub fn send_data_frame(
        &mut self,
        device_id: usize,
        data: &[u8],
        target_mac_address: [u8; 6],
        ether_type: u16,
    ) -> Result<(), ()> {
        self.ethernet_manager
            .send_data(device_id, data, target_mac_address, ether_type)
    }

    /// frame_info will be encapsulated.
    pub fn reply_data_frame(
        &mut self,
        frame_info: ethernet_device::EthernetFrameInfo,
        data: &[u8],
    ) -> Result<(), ()> {
        self.ethernet_manager.reply_data(frame_info, data)
    }
}
