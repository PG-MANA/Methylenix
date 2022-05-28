//!
//! UDP
//!

use super::{ipv4, AddressPrinter};

use crate::kernel::memory_manager::data_type::{Address, MSize, VAddress};
use crate::kernel::sync::spin_lock::IrqSaveSpinLockFlag;
//use crate::kernel::task_manager::ThreadEntry;

use crate::{kfree, kmalloc};

use crate::kernel::collections::ptr_linked_list::{PtrLinkedList, PtrLinkedListNode};
//use alloc::collections::LinkedList;

pub const UDP_HEADER_SIZE: usize = 0x08;
pub const IPV4_PROTOCOL_UDP: u8 = 0x11;

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
    if data.len() > u16::MAX as usize {
        return Err(());
    }
    let mut header = [0u8; UDP_HEADER_SIZE + ipv4::IPV4_DEFAULT_HEADER_SIZE];
    const UDP_HEADER_BASE: usize = ipv4::IPV4_DEFAULT_HEADER_SIZE;

    header[UDP_HEADER_BASE..=(UDP_HEADER_BASE + 1)].copy_from_slice(&sender_port.to_be_bytes());
    header[(UDP_HEADER_BASE + 2)..=(UDP_HEADER_BASE + 3)]
        .copy_from_slice(&destination_port.to_be_bytes());
    header[(UDP_HEADER_BASE + 4)..=(UDP_HEADER_BASE + 5)]
        .copy_from_slice(&(data.len() as u16).to_be_bytes());

    ipv4::create_default_ipv4_header(
        &mut header[0..UDP_HEADER_BASE],
        data.len(),
        packet_id,
        true,
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
    sender_ipv4_address: u32,
    target_ipv4_address: u32,
) {
    let udp_base = allocated_data_base.to_usize() + packet_offset;
    let sender_port = u16::from_be(unsafe { *(udp_base as *const u16) });
    let target_port = u16::from_be(unsafe { *((udp_base + 2) as *const u16) });
    let length = u16::from_be(unsafe { *((udp_base + 4) as *const u16) });
    if (length as usize) + packet_offset > data_length.to_usize() {
        pr_err!("Invalid payload size: {:#X}", length);
        let _ = kfree!(allocated_data_base, data_length);
        return;
    }
    pr_debug!(
        "UDP Packet: {{From: {}:{}, To: {}:{}, Length: {}}}",
        AddressPrinter {
            address: &sender_ipv4_address.to_be_bytes(),
            separator: '.',
            is_hex: false
        },
        sender_port,
        AddressPrinter {
            address: &target_ipv4_address.to_be_bytes(),
            separator: '.',
            is_hex: false
        },
        target_port,
        length
    );

    let _lock = unsafe { UDP_PORT_MANAGER.0.lock() };
    for e in unsafe {
        UDP_PORT_MANAGER
            .1
            .iter_mut(offset_of!(UdpPortListenEntry, list))
    } {
        if (e.from_ipv4_address == 0 || e.from_ipv4_address == sender_ipv4_address)
            && (e.to_ipv4_address == 0 || e.to_ipv4_address == target_ipv4_address)
            && (e.port == target_port)
        {
            let e = UdpPortListenEntry {
                list: PtrLinkedListNode::new(),
                entry: e.entry,
                from_ipv4_address: e.from_ipv4_address,
                to_ipv4_address: e.to_ipv4_address,
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
            address: &sender_ipv4_address.to_be_bytes(),
            separator: '.',
            is_hex: false
        },
        sender_port,
        AddressPrinter {
            address: &target_ipv4_address.to_be_bytes(),
            separator: '.',
            is_hex: false
        },
        target_port,
        length
    );

    let _ = kfree!(allocated_data_base, data_length);
}
