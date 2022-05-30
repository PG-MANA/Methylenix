//!
//! UDP
//!

use super::{ethernet_device::EthernetFrameInfo, ipv4, AddressPrinter};

use crate::kernel::memory_manager::data_type::{Address, MSize, VAddress};
use crate::kernel::sync::spin_lock::IrqSaveSpinLockFlag;
//use crate::kernel::task_manager::ThreadEntry;

use crate::{kfree, kmalloc};

use crate::kernel::collections::ptr_linked_list::{PtrLinkedList, PtrLinkedListNode};
//use alloc::collections::LinkedList;

pub const UDP_HEADER_SIZE: usize = 0x08;
pub const IPV4_PROTOCOL_UDP: u8 = 0x11;

#[repr(C)]
struct UdpPacket {
    sender_port: u16,
    destination_port: u16,
    packet_length: u16,
    checksum: u16,
}

#[allow(dead_code)]
impl UdpPacket {
    #[allow(dead_code)]
    pub const fn new(sender_port: u16, destination_port: u16, packet_length: u16) -> Self {
        assert!(core::mem::size_of::<Self>() == UDP_HEADER_SIZE);
        Self {
            sender_port: sender_port.to_be(),
            destination_port: destination_port.to_be(),
            packet_length: packet_length.to_be(),
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

    pub fn set_destination_port(&mut self, port: u16) {
        self.destination_port = port.to_be();
    }

    pub const fn get_packet_length(&self) -> u16 {
        u16::from_be(self.packet_length)
    }

    pub fn set_packet_length(&mut self, length: u16) {
        self.packet_length = length.to_be();
    }

    pub const fn get_checksum(&self) -> u16 {
        u16::from_be(self.checksum)
    }

    pub const fn set_checksum(&mut self, checksum: u16) {
        self.checksum = checksum.to_be();
    }
}

pub struct UdpPortListenEntry {
    //    pub thread: &'static mut ThreadEntry,
    list: PtrLinkedListNode<Self>,
    pub entry: fn(&Self),
    pub from_ipv4_address: u32,
    pub to_ipv4_address: u32,
    pub port: u16,
    pub allocated_data_address: VAddress,
    pub data_length: MSize,
    pub offset: MSize,
}

impl UdpPortListenEntry {
    pub fn new(entry: fn(&Self), from_ipv4_address: u32, to_ipv4_address: u32, port: u16) -> Self {
        Self {
            list: PtrLinkedListNode::new(),
            entry,
            from_ipv4_address,
            to_ipv4_address,
            port,
            allocated_data_address: VAddress::new(0),
            data_length: MSize::new(0),
            offset: MSize::new(0),
        }
    }
}

static mut UDP_PORT_MANAGER: (IrqSaveSpinLockFlag, PtrLinkedList<UdpPortListenEntry>) =
    (IrqSaveSpinLockFlag::new(), PtrLinkedList::new());

pub fn create_ipv4_udp_header(
    data: &[u8],
    sender_port: u16,
    sender_ipv4_address: u32,
    destination_port: u16,
    destination_ipv4_address: u32,
    packet_id: u16,
) -> Result<[u8; UDP_HEADER_SIZE + ipv4::IPV4_DEFAULT_HEADER_SIZE], ()> {
    if (data.len() + UDP_HEADER_SIZE) > u16::MAX as usize {
        return Err(());
    }
    let mut header = [0u8; UDP_HEADER_SIZE + ipv4::IPV4_DEFAULT_HEADER_SIZE];
    let (ipv4_header, udp_header) = header.split_at_mut(ipv4::IPV4_DEFAULT_HEADER_SIZE);
    let udp_header = UdpPacket::from_buffer(udp_header);

    udp_header.set_sender_port(sender_port);
    udp_header.set_destination_port(destination_port);
    udp_header.set_packet_length((data.len() + UDP_HEADER_SIZE) as u16);

    ipv4::create_default_ipv4_header(
        ipv4_header,
        udp_header.get_packet_length() as usize,
        packet_id,
        ipv4::get_default_ttl(),
        IPV4_PROTOCOL_UDP,
        sender_ipv4_address,
        destination_ipv4_address,
    )?;

    return Ok(header);
}

pub fn add_udp_port_listener(entry: UdpPortListenEntry) -> Result<(), ()> {
    /* TODO: avoid conflicting port */
    let entry = match kmalloc!(UdpPortListenEntry, entry) {
        Ok(a) => a,
        Err(e) => {
            pr_err!("Failed to allocate memory: {:?}", e);
            return Err(());
        }
    };

    let _lock = unsafe { UDP_PORT_MANAGER.0.lock() };
    unsafe { &mut UDP_PORT_MANAGER.1 }.insert_tail(&mut entry.list);
    return Ok(());
}

pub fn udp_ipv4_packet_handler(
    allocated_data_base: VAddress,
    data_length: MSize,
    packet_offset: usize,
    _frame_info: EthernetFrameInfo,
    ipv4_packet_info: ipv4::Ipv4PacketInfo,
) {
    let udp_base = allocated_data_base.to_usize() + packet_offset;
    if (packet_offset + UDP_HEADER_SIZE) > data_length.to_usize() {
        pr_err!("Invalid UDP packet");
        let _ = kfree!(allocated_data_base, data_length);
        return;
    }
    let udp_packet =
        UdpPacket::from_buffer(unsafe { &mut *(udp_base as *mut [u8; UDP_HEADER_SIZE]) });
    if (udp_packet.get_packet_length() as usize) + packet_offset > data_length.to_usize() {
        pr_err!(
            "Invalid payload size: {:#X}",
            udp_packet.get_packet_length()
        );
        let _ = kfree!(allocated_data_base, data_length);
        return;
    }

    let sender_port = udp_packet.get_sender_port();
    let destination_port = udp_packet.get_destination_port();

    pr_debug!(
        "UDP Packet: {{From: {}:{}, To: {}:{}, Length: {}}}",
        AddressPrinter {
            address: &ipv4_packet_info.get_sender_address().to_be_bytes(),
            separator: '.',
            is_hex: false
        },
        sender_port,
        AddressPrinter {
            address: &ipv4_packet_info.get_destination_address().to_be_bytes(),
            separator: '.',
            is_hex: false
        },
        destination_port,
        udp_packet.get_packet_length() - UDP_HEADER_SIZE as u16
    );

    let _lock = unsafe { UDP_PORT_MANAGER.0.lock() };
    for e in unsafe {
        UDP_PORT_MANAGER
            .1
            .iter_mut(offset_of!(UdpPortListenEntry, list))
    } {
        if (e.from_ipv4_address == 0
            || e.from_ipv4_address == ipv4_packet_info.get_sender_address())
            && (e.to_ipv4_address == 0
                || e.to_ipv4_address == ipv4_packet_info.get_destination_address())
            && (e.port == destination_port)
        {
            let e = UdpPortListenEntry {
                list: PtrLinkedListNode::new(),
                entry: e.entry,
                from_ipv4_address: ipv4_packet_info.get_sender_address(),
                to_ipv4_address: ipv4_packet_info.get_destination_address(),
                port: e.port,
                allocated_data_address: allocated_data_base,
                data_length,
                offset: MSize::new(UDP_HEADER_SIZE + packet_offset),
            };
            drop(_lock);
            (e.entry)(&e);
            return;
        }
    }

    pr_debug!(
        "Unprocessed UDP Packet: {{From: {}:{}, To: {}:{}, Length: {}}}",
        AddressPrinter {
            address: &ipv4_packet_info.get_sender_address().to_be_bytes(),
            separator: '.',
            is_hex: false
        },
        sender_port,
        AddressPrinter {
            address: &ipv4_packet_info.get_destination_address().to_be_bytes(),
            separator: '.',
            is_hex: false
        },
        destination_port,
        udp_packet.get_packet_length() - UDP_HEADER_SIZE as u16
    );

    let _ = kfree!(allocated_data_base, data_length);
}
