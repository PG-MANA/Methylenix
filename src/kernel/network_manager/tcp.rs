//!
//! TCP
//!

use super::{ethernet_device::EthernetFrameInfo, ipv4, AddressPrinter};

use crate::kernel::collections::ptr_linked_list::{PtrLinkedList, PtrLinkedListNode};
use crate::kernel::memory_manager::data_type::{Address, MSize, VAddress};
use crate::kernel::sync::spin_lock::{IrqSaveSpinLockFlag, SpinLockFlag};

use crate::{kfree, kmalloc};

use crate::kernel::manager_cluster::get_kernel_manager_cluster;

use core::ptr::NonNull;

use alloc::collections::LinkedList;

pub const IPV4_PROTOCOL_TCP: u8 = 0x06;

struct SessionBuffer {
    allocated_data_base: VAddress,
    data_length: MSize,
    payload_offset: usize,
    sequence_number: u32,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum TcpSessionStatus {
    HalfOpen,
    Open,
}

pub type TcpDataHandler = fn(
    data_base_address: VAddress,
    data_length: MSize,
    segment_info: TcpSegmentInfo,
) -> Result<(), ()>;

pub struct TcpSessionInfo {
    lock: SpinLockFlag,
    handler: TcpDataHandler,
    sender_port: u16,
    destination_port: u16,
    window_size: u16,
    buffer_list: LinkedList<SessionBuffer>,
    next_sequence_number: u32,
    last_acknowledge_number: u32,
    listen_entry: Option<NonNull<TcpPortListenEntry>>,
    status: TcpSessionStatus,
}

#[derive(Clone, Eq, PartialEq)]
pub enum AddressInfo {
    Any,
    Ipv4(u32),
    Ipv6([u8; 16]),
}

pub type TcpPortListenerHandler = fn(segment_info: TcpSegmentInfo) -> TcpDataHandler;

pub struct TcpPortListenEntry {
    lock: SpinLockFlag,
    handler: TcpPortListenerHandler,
    port: u16,
    acceptable_address: AddressInfo,
    max_acceptable_connection: usize,
    number_of_active_connection: usize,
}

pub struct TcpSegmentInfo {
    sender_port: u16,
    destination_port: u16,
    packet_info: ipv4::Ipv4PacketInfo,
}

impl TcpSegmentInfo {
    pub fn get_sender_port(&self) -> u16 {
        self.sender_port
    }

    pub fn get_destination_port(&self) -> u16 {
        self.destination_port
    }

    pub fn get_packet_info(&self) -> &ipv4::Ipv4PacketInfo {
        &self.packet_info
    }
}

#[repr(C)]
struct DefaultTcpSegment {
    sender_port: u16,
    destination_port: u16,
    sequence_number: u32,
    acknowledgement_number: u32,
    header_length_and_ns: u8,
    flags: u8,
    window_size: u16,
    checksum: u16,
    urgent_pointer: u16,
}

#[allow(dead_code)]
impl DefaultTcpSegment {
    pub fn from_buffer(buffer: &mut [u8]) -> &mut Self {
        assert!(buffer.len() >= core::mem::size_of::<Self>());
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

    pub const fn get_sequence_number(&self) -> u32 {
        u32::from_be(self.sequence_number)
    }

    pub fn set_sequence_number(&mut self, sequence_number: u32) {
        self.sequence_number = sequence_number.to_be();
    }

    pub const fn get_acknowledgement_number(&self) -> u32 {
        u32::from_be(self.acknowledgement_number)
    }

    pub fn set_acknowledgement_number(&mut self, acknowledgement_number: u32) {
        self.acknowledgement_number = acknowledgement_number.to_be();
    }

    pub const fn get_window_size(&self) -> u16 {
        u16::from_be(self.window_size)
    }

    pub fn set_window_size(&mut self, window_size: u16) {
        self.window_size = window_size.to_be();
    }

    #[allow(dead_code)]
    #[cfg(target_endian = "big")]
    pub const fn is_ns_active(&self) -> bool {
        (self.header_length_and_ns >> 7) != 0
    }

    #[allow(dead_code)]
    #[cfg(target_endian = "little")]
    pub const fn is_ns_active(&self) -> bool {
        (self.header_length_and_ns & 1) != 0
    }

    #[cfg(target_endian = "big")]
    pub const fn get_header_length(&self) -> usize {
        (self.header_length_and_ns & 0xf) as usize * 4
    }

    #[cfg(target_endian = "little")]
    pub const fn get_header_length(&self) -> usize {
        (self.header_length_and_ns >> 4) as usize * 4
    }

    #[cfg(target_endian = "big")]
    pub const fn set_header_length(&mut self, length: u8) {
        self.header_length_and_ns = (self.header_length_and_ns & !0xf) | (length >> 2)
    }

    #[cfg(target_endian = "little")]
    pub const fn set_header_length(&mut self, length: u8) {
        self.header_length_and_ns = (self.header_length_and_ns & !0xf0) | ((length >> 2) << 4)
    }

    #[cfg(target_endian = "big")]
    pub const fn is_fin_active(&self) -> bool {
        (self.flags >> 7) != 0
    }

    #[cfg(target_endian = "little")]
    pub const fn is_fin_active(&self) -> bool {
        (self.flags & 1) != 0
    }

    #[cfg(target_endian = "big")]
    pub const fn is_syn_active(&self) -> bool {
        ((self.flags >> 6) & 1) != 0
    }

    #[cfg(target_endian = "little")]
    pub const fn is_syn_active(&self) -> bool {
        (self.flags & (1 << 1)) != 0
    }

    #[cfg(target_endian = "big")]
    pub const fn is_ack_active(&self) -> bool {
        ((self.flags >> 3) & 1) != 0
    }

    #[cfg(target_endian = "little")]
    pub const fn is_ack_active(&self) -> bool {
        (self.flags & (1 << 4)) != 0
    }

    #[cfg(target_endian = "big")]
    pub const fn set_syn_active(&mut self) {
        self.flags |= 1 << 6
    }

    #[cfg(target_endian = "little")]
    pub const fn set_syn_active(&mut self) {
        self.flags |= 1 << 1
    }

    #[cfg(target_endian = "big")]
    pub const fn set_ack_active(&mut self) {
        self.flags |= 1 << 3;
    }

    #[cfg(target_endian = "little")]
    pub const fn set_ack_active(&mut self) {
        self.flags |= 1 << 4
    }
    pub const fn get_checksum(&self) -> u16 {
        u16::from_be(self.checksum)
    }

    pub fn set_checksum_ipv4(
        &mut self,
        sender_ipv4_address: u32,
        destination_ipv4_address: u32,
        tcp_header_length: u16,
        data: &[u8],
    ) {
        self.checksum = 0;
        let mut checksum: u16 = 0;
        let sender_ipv4_address = sender_ipv4_address.to_be_bytes();
        let destination_ipv4_address = destination_ipv4_address.to_be_bytes();
        let packet_length = (data.len() as u16 + tcp_header_length).to_be_bytes();
        let pre_header: [u8; 12] = [
            sender_ipv4_address[0],
            sender_ipv4_address[1],
            sender_ipv4_address[2],
            sender_ipv4_address[3],
            destination_ipv4_address[0],
            destination_ipv4_address[1],
            destination_ipv4_address[2],
            destination_ipv4_address[3],
            0x00,
            IPV4_PROTOCOL_TCP,
            packet_length[0],
            packet_length[1],
        ];

        let calc_checksum = |buffer: &[u8], checksum: &mut u16| {
            for i in 0..((buffer.len()) / core::mem::size_of::<u16>()) {
                let i = checksum
                    .overflowing_add(u16::from_be_bytes([buffer[2 * i], buffer[2 * i + 1]]));
                *checksum = i.0 + (i.1 as u16);
            }
            if (buffer.len() & 1) != 0 {
                let i = checksum.overflowing_add(u16::from_be_bytes([buffer[buffer.len() - 1], 0]));
                *checksum = i.0 + (i.1 as u16);
            }
        };

        calc_checksum(&pre_header, &mut checksum);
        let header_buffer = unsafe {
            core::slice::from_raw_parts(
                self as *const _ as usize as *const u8,
                tcp_header_length as usize,
            )
        };
        calc_checksum(header_buffer, &mut checksum);
        calc_checksum(data, &mut checksum);
        self.checksum = (!checksum).to_be();
    }
}

static mut TCP_BIND_MANAGER: (IrqSaveSpinLockFlag, LinkedList<TcpPortListenEntry>) =
    (IrqSaveSpinLockFlag::new(), LinkedList::new());

static mut TCP_SESSION_MANAGER: (IrqSaveSpinLockFlag, LinkedList<TcpSessionInfo>) =
    (IrqSaveSpinLockFlag::new(), LinkedList::new());

fn send_ack_syn_ipv4(
    acknowledgement_number: u32,
    sequence_number: u32,
    destination_port: u16,
    sender_port: u16,
    window_size: u16,
    packet_info: ipv4::Ipv4PacketInfo,
    frame_info: EthernetFrameInfo,
) -> Result<(), ()> {
    const HEADER_SIZE: usize = core::mem::size_of::<DefaultTcpSegment>();
    let mut header = [0u8; HEADER_SIZE + ipv4::IPV4_DEFAULT_HEADER_SIZE];
    let (ipv4_header, tcp_header) = header.split_at_mut(ipv4::IPV4_DEFAULT_HEADER_SIZE);
    let tcp_segment = DefaultTcpSegment::from_buffer(tcp_header);

    tcp_segment.set_header_length(HEADER_SIZE as u8);
    tcp_segment.set_ack_active();
    tcp_segment.set_syn_active();
    tcp_segment.set_destination_port(destination_port);
    tcp_segment.set_sender_port(sender_port);
    tcp_segment.set_acknowledgement_number(acknowledgement_number);
    tcp_segment.set_sequence_number(sequence_number);
    tcp_segment.set_window_size(window_size);
    tcp_segment.set_checksum_ipv4(
        packet_info.get_destination_address(),
        packet_info.get_sender_address(),
        HEADER_SIZE as u16,
        &[],
    );

    ipv4::create_default_ipv4_header(
        ipv4_header,
        HEADER_SIZE as usize,
        0,
        ipv4::get_default_ttl(),
        IPV4_PROTOCOL_TCP,
        packet_info.get_destination_address(),
        packet_info.get_sender_address(),
    )?;
    get_kernel_manager_cluster()
        .ethernet_device_manager
        .reply_data(frame_info, &header)
}

fn send_ack_ipv4(
    acknowledgement_number: u32,
    sequence_number: u32,
    destination_port: u16,
    sender_port: u16,
    window_size: u16,
    packet_info: ipv4::Ipv4PacketInfo,
    frame_info: EthernetFrameInfo,
) -> Result<(), ()> {
    const HEADER_SIZE: usize = core::mem::size_of::<DefaultTcpSegment>();
    let mut header = [0u8; HEADER_SIZE + ipv4::IPV4_DEFAULT_HEADER_SIZE];
    let (ipv4_header, tcp_header) = header.split_at_mut(ipv4::IPV4_DEFAULT_HEADER_SIZE);
    let tcp_segment = DefaultTcpSegment::from_buffer(tcp_header);

    tcp_segment.set_header_length(HEADER_SIZE as u8);
    tcp_segment.set_ack_active();
    tcp_segment.set_destination_port(destination_port);
    tcp_segment.set_sender_port(sender_port);
    tcp_segment.set_acknowledgement_number(acknowledgement_number);
    tcp_segment.set_sequence_number(sequence_number);
    tcp_segment.set_window_size(window_size);
    tcp_segment.set_checksum_ipv4(
        packet_info.get_destination_address(),
        packet_info.get_sender_address(),
        HEADER_SIZE as u16,
        &[],
    );

    ipv4::create_default_ipv4_header(
        ipv4_header,
        HEADER_SIZE as usize,
        0,
        ipv4::get_default_ttl(),
        IPV4_PROTOCOL_TCP,
        packet_info.get_destination_address(),
        packet_info.get_sender_address(),
    )?;
    get_kernel_manager_cluster()
        .ethernet_device_manager
        .reply_data(frame_info, &header)
}

pub fn bind_port(
    port: u16,
    acceptable_address: AddressInfo,
    max_acceptable_connection: usize,
    handler: TcpPortListenerHandler,
) -> Result<(), ()> {
    /* TODO: port check */
    let entry = TcpPortListenEntry {
        lock: SpinLockFlag::new(),
        port,
        acceptable_address,
        max_acceptable_connection,
        handler,
        number_of_active_connection: 0,
    };
    let _lock = unsafe { TCP_BIND_MANAGER.0.lock() };
    unsafe { TCP_BIND_MANAGER.1.push_back(entry) };
    return Ok(());
}

fn receive_packet_handler(
    allocated_data_base: VAddress,
    data_length: MSize,
    packet_offset: usize,
    frame_info: EthernetFrameInfo,
    ipv4_packet_info: ipv4::Ipv4PacketInfo,
    tcp_segment_header: &DefaultTcpSegment,
    e: &mut TcpSessionInfo,
) {
    let data_size =
        (data_length.to_usize() - packet_offset - tcp_segment_header.get_header_length());
    if data_size > u16::MAX as usize {
        pr_err!("Invalid packet size");
        let _ = kfree!(allocated_data_base, data_length);
        return;
    }
    let _lock = e.lock.lock();
    if tcp_segment_header.get_sequence_number() == e.next_sequence_number {
        /* Arrived next data */
        if let Err(err) = (e.handler)(
            allocated_data_base
                + MSize::new(packet_offset + tcp_segment_header.get_header_length()),
            MSize::new(data_size),
            TcpSegmentInfo {
                sender_port: tcp_segment_header.get_sender_port(),
                destination_port: tcp_segment_header.get_destination_port(),
                packet_info: ipv4_packet_info.clone(),
            },
        ) {
            pr_err!("Failed to notify data");
        }
        let _ = kfree!(allocated_data_base, data_length);

        if e.buffer_list.len() != 0 {
            /* The packets are not arrived sequentially */
            let mut next_sequence_number = tcp_segment_header
                .get_sequence_number()
                .overflowing_add(data_size as u32)
                .0;
            'outer_loop: loop {
                let mut cursor = e.buffer_list.cursor_front_mut();
                while let Some(buffer_entry) = cursor.current() {
                    if buffer_entry.sequence_number == next_sequence_number {
                        let buffer_data_size =
                            buffer_entry.data_length.to_usize() - buffer_entry.payload_offset;
                        next_sequence_number =
                            next_sequence_number.overflowing_add(data_size as u32).0;
                        if let Err(err) = (e.handler)(
                            buffer_entry.allocated_data_base
                                + MSize::new(buffer_entry.payload_offset),
                            MSize::new(buffer_data_size),
                            TcpSegmentInfo {
                                sender_port: tcp_segment_header.get_sender_port(),
                                destination_port: tcp_segment_header.get_destination_port(),
                                packet_info: ipv4_packet_info.clone(),
                            },
                        ) {
                            pr_err!("Failed to notify data");
                        }
                        let _ = kfree!(buffer_entry.allocated_data_base, buffer_entry.data_length);
                        cursor.remove_current();
                        continue 'outer_loop;
                    }
                    cursor.move_next();
                }
                break;
            }
        }
    } else {
        /* Arrived the data after of the next data */
        e.buffer_list.push_back(SessionBuffer {
            allocated_data_base,
            data_length,
            payload_offset: packet_offset + tcp_segment_header.get_header_length(),
            sequence_number: tcp_segment_header.get_sequence_number(),
        });
    }
    e.last_acknowledge_number = tcp_segment_header.get_acknowledgement_number();
    drop(_lock);
    /* Send ACK */
    if let Err(e) = send_ack_ipv4(
        tcp_segment_header
            .get_sequence_number()
            .overflowing_add(data_size as u32)
            .0,
        tcp_segment_header.get_acknowledgement_number(),
        tcp_segment_header.get_sender_port(),
        tcp_segment_header.get_destination_port(),
        tcp_segment_header.get_window_size(),
        ipv4_packet_info,
        frame_info,
    ) {
        pr_err!("Failed to send ACK:{:?}", e);
    }
}

pub fn tcp_ipv4_packet_handler(
    allocated_data_base: VAddress,
    data_length: MSize,
    packet_offset: usize,
    frame_info: EthernetFrameInfo,
    ipv4_packet_info: ipv4::Ipv4PacketInfo,
) {
    const TCP_DEFAULT_PACKET_SIZE: usize = core::mem::size_of::<DefaultTcpSegment>();
    if (packet_offset + TCP_DEFAULT_PACKET_SIZE) > data_length.to_usize() {
        pr_err!("Invalid TCP segment");
        let _ = kfree!(allocated_data_base, data_length);
        return;
    }
    let tcp_segment = DefaultTcpSegment::from_buffer(unsafe {
        &mut *((allocated_data_base.to_usize() + packet_offset)
            as *mut [u8; TCP_DEFAULT_PACKET_SIZE])
    });

    let sender_port = tcp_segment.get_sender_port();
    let destination_port = tcp_segment.get_destination_port();

    pr_debug!(
        "TCP Segment: {{From: {}:{}, To: {}:{}, Length: {}, HeaderLength: {}, ACK: {}, SYN: {}, FIN: {} }}",
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
        (data_length.to_usize() - packet_offset - tcp_segment.get_header_length()),
        tcp_segment.get_header_length(),
        tcp_segment.is_ack_active(),
        tcp_segment.is_syn_active(),
        tcp_segment.is_fin_active()
    );

    if tcp_segment.is_syn_active() {
        if tcp_segment.is_ack_active() {
            /* TODO: ... */
        } else {
            /* Connection from client */
            let _lock = unsafe { TCP_BIND_MANAGER.0.lock() };
            let list = unsafe { &mut TCP_BIND_MANAGER.1 };
            for e in list.iter_mut() {
                if e.port == destination_port
                    && (e.acceptable_address == AddressInfo::Any
                        || e.acceptable_address
                            == AddressInfo::Ipv4(ipv4_packet_info.get_destination_address()))
                {
                    drop(_lock);
                    let _entry_lock = e.lock.lock();
                    if e.number_of_active_connection < e.max_acceptable_connection {
                        e.number_of_active_connection += 1;
                        let sequence_number = 1; /* TODO: randomise */
                        let session_entry = TcpSessionInfo {
                            lock: SpinLockFlag::new(),
                            sender_port,
                            destination_port,
                            window_size: tcp_segment.get_window_size(),
                            buffer_list: LinkedList::new(),
                            next_sequence_number: sequence_number + 1,
                            last_acknowledge_number: 0,
                            listen_entry: NonNull::new(e),
                            status: TcpSessionStatus::HalfOpen,
                            handler: (e.handler)(TcpSegmentInfo {
                                sender_port,
                                destination_port,
                                packet_info: ipv4_packet_info.clone(),
                            }),
                        };
                        let _session_lock = unsafe { TCP_SESSION_MANAGER.0.lock() };
                        unsafe { TCP_SESSION_MANAGER.1.push_front(session_entry) };
                        drop(_session_lock);
                        if let Err(err) = send_ack_syn_ipv4(
                            tcp_segment.get_sequence_number() + 1,
                            sequence_number,
                            tcp_segment.get_sender_port(),
                            tcp_segment.get_destination_port(),
                            tcp_segment.get_window_size(),
                            ipv4_packet_info.clone(),
                            frame_info,
                        ) {
                            pr_err!("Failed to send SYN-ACK: {:?}", err);
                        }
                    } else {
                        /* drop the segment */
                    }
                    break;
                }
            }
            let _ = kfree!(allocated_data_base, data_length);
        }
    } else if tcp_segment.is_ack_active() {
        let _session_lock = unsafe { TCP_SESSION_MANAGER.0.lock() };
        for e in unsafe { TCP_SESSION_MANAGER.1.iter_mut() } {
            if e.destination_port == destination_port && e.sender_port == sender_port {
                if e.status == TcpSessionStatus::HalfOpen {
                    if tcp_segment.get_acknowledgement_number() == e.next_sequence_number {
                        e.status = TcpSessionStatus::Open;
                        e.next_sequence_number = tcp_segment.get_sequence_number();
                        e.last_acknowledge_number = tcp_segment.get_acknowledgement_number();
                    }
                } else {
                    drop(_session_lock);
                    if tcp_segment.get_header_length() == (data_length.to_usize() - packet_offset) {
                        /* ACK only */
                        todo!();
                        return;
                    } else {
                        return receive_packet_handler(
                            allocated_data_base,
                            data_length,
                            packet_offset,
                            frame_info,
                            ipv4_packet_info,
                            tcp_segment,
                            e,
                        );
                    }
                }
            }
        }
        drop(_session_lock);
        let _ = kfree!(allocated_data_base, data_length);
    } else {
        let _session_lock = unsafe { TCP_SESSION_MANAGER.0.lock() };
        for e in unsafe { TCP_SESSION_MANAGER.1.iter_mut() } {
            if e.destination_port == destination_port && e.sender_port == sender_port {
                drop(_session_lock);
                return receive_packet_handler(
                    allocated_data_base,
                    data_length,
                    packet_offset,
                    frame_info,
                    ipv4_packet_info,
                    tcp_segment,
                    e,
                );
            }
        }
        drop(_session_lock);
        let _ = kfree!(allocated_data_base, data_length);
    }
}
