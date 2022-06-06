//!
//! TCP
//!

use super::{ethernet_device::EthernetFrameInfo, ipv4, AddressInfo, AddressPrinter};

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{Address, MSize, VAddress};
use crate::kernel::sync::spin_lock::{IrqSaveSpinLockFlag, SpinLockFlag};

use crate::{kfree, kmalloc};

use alloc::collections::LinkedList;

pub const IPV4_PROTOCOL_TCP: u8 = 0x06;
pub const MAX_SEGMENT_SIZE: usize = 1460;
pub const MAX_TRANSMISSION_UNIT: usize = 1500;

struct SessionBuffer {
    allocated_data_base: VAddress,
    data_length: MSize,
    payload_offset: usize,
    sequence_number: u32,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum TcpSessionStatus {
    HalfOpened,
    Opened,
    OpenOppositeClosed,
    Closing,
    ClosingOppositeClosed,
    ClosedOppositeOpened,
    ClosedOppositeClosing,
    /* Sent ACK for FIN, when nothing comes after several time, become closed  */
    Closed,
}

pub type TcpDataHandler = fn(
    data_base_address: VAddress,
    data_length: MSize,
    segment_info: TcpSegmentInfo,
) -> Result<(), ()>;

pub struct TcpSessionInfo {
    lock: SpinLockFlag,
    handler: TcpDataHandler,
    //event_handler,
    /// segment_info is **the arrived segment information**.
    /// the **sender** means **opposite**, and the **destination** means **our side**.
    /// Be careful when reply data.
    segment_info: TcpSegmentInfo,
    window_size: u16,
    buffer_list: LinkedList<SessionBuffer>,
    expected_arrive_sequence_number: u32,
    next_sequence_number: u32,
    last_sent_acknowledge_number: u32,
    status: TcpSessionStatus,
}

impl TcpSessionInfo {
    fn free_buffer(&mut self) {
        assert!(self.lock.is_locked());
        while let Some(e) = self.buffer_list.pop_front() {
            if !e.data_length.is_zero() {
                let _ = kfree!(e.allocated_data_base, e.data_length);
            }
        }
    }
}

pub type TcpPortListenerHandler = fn(segment_info: TcpSegmentInfo) -> Result<TcpDataHandler, ()>;

pub struct TcpPortListenEntry {
    lock: SpinLockFlag,
    handler: TcpPortListenerHandler,
    port: u16,
    acceptable_address: AddressInfo,
    max_acceptable_connection: usize,
    number_of_active_connection: usize,
}

#[derive(Clone)]
pub struct TcpSegmentInfo {
    sender_port: u16,
    destination_port: u16,
    packet_info: ipv4::Ipv4PacketInfo,
    frame_info: EthernetFrameInfo,
}

impl TcpSegmentInfo {
    pub fn get_sender_port(&self) -> u16 {
        self.sender_port
    }

    pub fn get_destination_port(&self) -> u16 {
        self.destination_port
    }

    pub fn get_their_port(&self) -> u16 {
        self.get_sender_port()
    }

    pub fn get_our_port(&self) -> u16 {
        self.get_destination_port()
    }

    pub fn get_packet_info(&self) -> &ipv4::Ipv4PacketInfo {
        &self.packet_info
    }

    pub fn is_equal(&self, other: &Self) -> bool {
        self.get_sender_port() == other.get_sender_port()
            && self.get_destination_port() == other.get_destination_port()
            && self.get_packet_info().get_destination_address()
                == other.get_packet_info().get_destination_address()
            && self.get_packet_info().get_sender_address()
                == other.get_packet_info().get_sender_address()
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
    pub const fn set_fin_active(&mut self) {
        self.flags |= 1 << 7
    }

    #[cfg(target_endian = "little")]
    pub const fn set_fin_active(&mut self) {
        self.flags |= 1 << 0
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
    pub const fn set_syn_active(&mut self) {
        self.flags |= 1 << 6
    }

    #[cfg(target_endian = "little")]
    pub const fn set_syn_active(&mut self) {
        self.flags |= 1 << 1
    }

    #[cfg(target_endian = "big")]
    pub const fn is_rst_active(&self) -> bool {
        ((self.flags >> 5) & 1) != 0
    }

    #[cfg(target_endian = "little")]
    pub const fn is_rst_active(&self) -> bool {
        (self.flags & (1 << 2)) != 0
    }

    #[cfg(target_endian = "big")]
    pub const fn set_rst_active(&mut self) {
        self.flags |= 1 << 5
    }

    #[cfg(target_endian = "little")]
    pub const fn set_rst_active(&mut self) {
        self.flags |= 1 << 2
    }

    #[cfg(target_endian = "big")]
    pub const fn is_psh_active(&self) -> bool {
        ((self.flags >> 4) & 1) != 0
    }

    #[cfg(target_endian = "little")]
    pub const fn is_psh_active(&self) -> bool {
        (self.flags & (1 << 3)) != 0
    }

    #[cfg(target_endian = "big")]
    pub const fn set_psh_active(&mut self) {
        self.flags |= 1 << 4
    }

    #[cfg(target_endian = "little")]
    pub const fn set_psh_active(&mut self) {
        self.flags |= 1 << 3
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

fn reply_ack_ipv4(
    acknowledgement_number: u32,
    sequence_number: u32,
    window_size: u16,
    segment_info: &TcpSegmentInfo,
    is_syn_active: bool,
) -> Result<(), ()> {
    const HEADER_SIZE: usize = core::mem::size_of::<DefaultTcpSegment>();
    let mut header = [0u8; HEADER_SIZE + ipv4::IPV4_DEFAULT_HEADER_SIZE];
    let (ipv4_header, tcp_header) = header.split_at_mut(ipv4::IPV4_DEFAULT_HEADER_SIZE);
    let tcp_segment = DefaultTcpSegment::from_buffer(tcp_header);

    tcp_segment.set_header_length(HEADER_SIZE as u8);
    tcp_segment.set_ack_active();
    if is_syn_active {
        tcp_segment.set_syn_active();
    }
    tcp_segment.set_destination_port(segment_info.get_their_port());
    tcp_segment.set_sender_port(segment_info.get_our_port());
    tcp_segment.set_acknowledgement_number(acknowledgement_number);
    tcp_segment.set_sequence_number(sequence_number);
    tcp_segment.set_window_size(window_size);
    tcp_segment.set_checksum_ipv4(
        segment_info.get_packet_info().get_our_address(),
        segment_info.get_packet_info().get_their_address(),
        HEADER_SIZE as u16,
        &[],
    );

    ipv4::create_default_ipv4_header(
        ipv4_header,
        HEADER_SIZE as usize,
        0,
        ipv4::get_default_ttl(),
        IPV4_PROTOCOL_TCP,
        segment_info.get_packet_info().get_our_address(),
        segment_info.get_packet_info().get_their_address(),
    )?;
    get_kernel_manager_cluster()
        .network_manager
        .reply_data_frame(segment_info.frame_info.clone(), &header)
}

fn send_fin_ipv4(
    acknowledgement_number: u32,
    sequence_number: u32,
    window_size: u16,
    segment_info: &TcpSegmentInfo,
) -> Result<(), ()> {
    const HEADER_SIZE: usize = core::mem::size_of::<DefaultTcpSegment>();
    let mut header = [0u8; HEADER_SIZE + ipv4::IPV4_DEFAULT_HEADER_SIZE];
    let (ipv4_header, tcp_header) = header.split_at_mut(ipv4::IPV4_DEFAULT_HEADER_SIZE);
    let tcp_segment = DefaultTcpSegment::from_buffer(tcp_header);

    tcp_segment.set_header_length(HEADER_SIZE as u8);
    tcp_segment.set_ack_active();
    tcp_segment.set_fin_active();
    tcp_segment.set_destination_port(segment_info.get_their_port());
    tcp_segment.set_sender_port(segment_info.get_our_port());
    tcp_segment.set_acknowledgement_number(acknowledgement_number);
    tcp_segment.set_sequence_number(sequence_number);
    tcp_segment.set_window_size(window_size);
    tcp_segment.set_checksum_ipv4(
        segment_info.get_packet_info().get_our_address(),
        segment_info.get_packet_info().get_their_address(),
        HEADER_SIZE as u16,
        &[],
    );

    ipv4::create_default_ipv4_header(
        ipv4_header,
        HEADER_SIZE as usize,
        0,
        ipv4::get_default_ttl(),
        IPV4_PROTOCOL_TCP,
        segment_info.get_packet_info().get_our_address(),
        segment_info.get_packet_info().get_their_address(),
    )?;
    get_kernel_manager_cluster()
        .network_manager
        .reply_data_frame(segment_info.frame_info.clone(), &header)
}

fn send_data_ipv4(
    temporary_buffer: &mut [u8],
    data_address: VAddress,
    data_size: MSize,
    acknowledgement_number: u32,
    sequence_number: u32,
    window_size: u16,
    segment_info: &TcpSegmentInfo,
) -> Result<(), ()> {
    const HEADER_SIZE: usize = core::mem::size_of::<DefaultTcpSegment>();
    let mut header = [0u8; HEADER_SIZE + ipv4::IPV4_DEFAULT_HEADER_SIZE];
    if temporary_buffer.len() < header.len() + data_size.to_usize() {
        return Err(());
    }

    let (ipv4_header, tcp_header) = header.split_at_mut(ipv4::IPV4_DEFAULT_HEADER_SIZE);
    let tcp_segment = DefaultTcpSegment::from_buffer(tcp_header);

    tcp_segment.set_header_length(HEADER_SIZE as u8);
    tcp_segment.set_psh_active();
    tcp_segment.set_ack_active();
    tcp_segment.set_destination_port(segment_info.get_their_port());
    tcp_segment.set_sender_port(segment_info.get_our_port());
    tcp_segment.set_acknowledgement_number(acknowledgement_number);
    tcp_segment.set_sequence_number(sequence_number);
    tcp_segment.set_window_size(window_size);
    tcp_segment.set_checksum_ipv4(
        segment_info.packet_info.get_our_address(),
        segment_info.packet_info.get_their_address(),
        HEADER_SIZE as u16,
        unsafe {
            core::slice::from_raw_parts(data_address.to_usize() as *const u8, data_size.to_usize())
        },
    );

    ipv4::create_default_ipv4_header(
        ipv4_header,
        HEADER_SIZE as usize + data_size.to_usize(),
        0,
        ipv4::get_default_ttl(),
        IPV4_PROTOCOL_TCP,
        segment_info.packet_info.get_our_address(),
        segment_info.packet_info.get_their_address(),
    )?;
    temporary_buffer[0..header.len()].copy_from_slice(&header);
    unsafe {
        core::ptr::copy_nonoverlapping(
            data_address.to_usize() as *const u8,
            temporary_buffer[header.len()..].as_mut_ptr(),
            data_size.to_usize(),
        )
    };
    let buffer = &temporary_buffer[0..(header.len() + data_size.to_usize())];
    get_kernel_manager_cluster()
        .network_manager
        .reply_data_frame(segment_info.frame_info.clone(), buffer)
}

pub fn send_data(
    segment_info: &TcpSegmentInfo,
    data_address: VAddress,
    data_size: MSize,
) -> Result<(), ()> {
    if data_size.is_zero() {
        return Err(());
    }
    let _session_lock = unsafe { TCP_SESSION_MANAGER.0.lock() };
    let mut cursor = unsafe { TCP_SESSION_MANAGER.1.cursor_front_mut() };
    while let Some(e) = cursor.current() {
        if e.segment_info.is_equal(segment_info) {
            let _lock = e.lock.lock();
            drop(_session_lock);
            let number_of_segments = (data_size.to_usize() - 1) / MAX_SEGMENT_SIZE + 1;
            let temporary_buffer = match kmalloc!(MSize::new(MAX_TRANSMISSION_UNIT)) {
                Ok(a) => a,
                Err(err) => {
                    pr_err!("Failed to allocate memory: {:?}", err);
                    return Err(());
                }
            };
            let buffer = unsafe {
                core::slice::from_raw_parts_mut(
                    temporary_buffer.to_usize() as *mut u8,
                    MAX_TRANSMISSION_UNIT,
                )
            };

            for i in 0..number_of_segments {
                let base = MSize::new(i * MAX_SEGMENT_SIZE);
                let send_size = (data_size - base).min(MSize::new(MAX_SEGMENT_SIZE));
                pr_debug!(
                    "Next Sequence: {}, Ack: {}",
                    e.next_sequence_number,
                    e.last_sent_acknowledge_number
                );
                pr_debug!("Send Size: {}", send_size.to_usize());
                send_data_ipv4(
                    buffer,
                    data_address + base,
                    send_size,
                    e.last_sent_acknowledge_number,
                    e.next_sequence_number,
                    e.window_size,
                    segment_info,
                )?;
                e.next_sequence_number = e
                    .next_sequence_number
                    .overflowing_add(send_size.to_usize() as u32)
                    .0;
                pr_debug!(
                    "Next Sequence: {}, Ack: {}",
                    e.next_sequence_number,
                    e.last_sent_acknowledge_number
                );
            }
            let _ = kfree!(temporary_buffer, MSize::new(MAX_TRANSMISSION_UNIT));
            return Ok(());
        }
        cursor.move_next();
    }
    return Err(());
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

pub fn unbind_port(port: u16, acceptable_address: AddressInfo) -> Result<(), ()> {
    let _lock = unsafe { TCP_BIND_MANAGER.0.lock() };
    let mut cursor = unsafe { TCP_BIND_MANAGER.1.cursor_front_mut() };
    while let Some(e) = cursor.current() {
        if e.port == port && e.acceptable_address == acceptable_address {
            cursor.remove_current();
            return Ok(());
        }
        cursor.move_next();
    }
    return Err(());
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
    let data_size = data_length.to_usize() - packet_offset - tcp_segment_header.get_header_length();
    if data_size > u16::MAX as usize {
        pr_err!("Invalid packet size");
        let _ = kfree!(allocated_data_base, data_length);
        return;
    }
    let _lock = e.lock.lock();
    if tcp_segment_header.get_sequence_number() == e.expected_arrive_sequence_number {
        let segment_info = TcpSegmentInfo {
            sender_port: tcp_segment_header.get_sender_port(),
            destination_port: tcp_segment_header.get_destination_port(),
            packet_info: ipv4_packet_info.clone(),
            frame_info: frame_info.clone(),
        };
        /* Arrived next data */
        if let Err(err) = (e.handler)(
            allocated_data_base
                + MSize::new(packet_offset + tcp_segment_header.get_header_length()),
            MSize::new(data_size),
            segment_info.clone(),
        ) {
            pr_err!("Failed to notify data: {:?}", err);
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
                            segment_info.clone(),
                        ) {
                            pr_err!("Failed to notify data: {:?}", err);
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
        e.expected_arrive_sequence_number = tcp_segment_header
            .get_sequence_number()
            .overflowing_add(data_size as u32)
            .0;
    } else {
        /* Arrived the data after of the next data */
        e.buffer_list.push_back(SessionBuffer {
            allocated_data_base,
            data_length,
            payload_offset: packet_offset + tcp_segment_header.get_header_length(),
            sequence_number: tcp_segment_header.get_sequence_number(),
        });
    }
    e.last_sent_acknowledge_number = tcp_segment_header
        .get_sequence_number()
        .overflowing_add(data_size as u32)
        .0;
    //e.last_sent_sequence_number = tcp_segment_header.get_acknowledgement_number();

    drop(_lock);

    /* Send ACK */
    if let Err(e) = reply_ack_ipv4(
        e.last_sent_acknowledge_number,
        e.next_sequence_number,
        tcp_segment_header.get_window_size(),
        &e.segment_info,
        false,
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
        pr_warn!("Invalid TCP segment");
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
        "TCP Segment: {{From: {}:{}, To: {}:{}, Length: {}, HeaderLength: {}, ACK: {}, SYN: {}, RST: {}, FIN: {} }}",
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
        tcp_segment.is_rst_active(),
        tcp_segment.is_fin_active()
    );

    if tcp_segment.is_fin_active() {
        let _session_lock = unsafe { TCP_SESSION_MANAGER.0.lock() };
        let mut cursor = unsafe { TCP_SESSION_MANAGER.1.cursor_front_mut() };
        while let Some(e) = cursor.current() {
            if e.segment_info.get_destination_port() == destination_port
                && e.segment_info.get_sender_port() == sender_port
            {
                let mut should_delete = false;
                let mut should_send_ack = true;
                let _lock = e.lock.lock();
                match e.status {
                    TcpSessionStatus::Closed => {
                        pr_debug!("Closed");
                        should_delete = true;
                    }
                    TcpSessionStatus::Closing => {
                        pr_debug!(
                            "Arrived Ack: {:#X}, Expected Ack: {:#X}",
                            tcp_segment.get_acknowledgement_number(),
                            e.next_sequence_number
                        );
                        if tcp_segment.is_ack_active() {
                            if tcp_segment.get_acknowledgement_number() == e.next_sequence_number {
                                e.status = TcpSessionStatus::ClosedOppositeClosing;
                            } else {
                                should_send_ack = false;
                            }
                        } else {
                            e.status = TcpSessionStatus::ClosingOppositeClosed;
                        }
                    }
                    TcpSessionStatus::HalfOpened => {
                        if let Err(err) = send_fin_ipv4(
                            e.last_sent_acknowledge_number,
                            e.next_sequence_number,
                            e.window_size,
                            &e.segment_info,
                        ) {
                            pr_err!("Failed to send FIN: {:?}", err);
                        }
                        e.status = TcpSessionStatus::ClosingOppositeClosed;
                    }
                    TcpSessionStatus::Opened | TcpSessionStatus::OpenOppositeClosed => {
                        e.status = TcpSessionStatus::OpenOppositeClosed;
                    }
                    TcpSessionStatus::ClosingOppositeClosed
                    | TcpSessionStatus::ClosedOppositeClosing => {
                        /* Reset ACK */
                        if let Err(err) = reply_ack_ipv4(
                            e.last_sent_acknowledge_number,
                            e.next_sequence_number,
                            e.window_size,
                            &e.segment_info,
                            false,
                        ) {
                            pr_err!("Failed to send ACK: {:?}", err);
                        }
                        should_send_ack = false;
                    }
                    TcpSessionStatus::ClosedOppositeOpened => {
                        pr_debug!("Opposite closing");
                        e.status = TcpSessionStatus::ClosedOppositeClosing;
                    }
                }
                if should_send_ack {
                    pr_debug!(
                        "Our: {}, Correct: {}",
                        e.last_sent_acknowledge_number.overflowing_add(1).0,
                        tcp_segment.get_sequence_number().overflowing_add(1).0
                    );

                    e.last_sent_acknowledge_number =
                        tcp_segment.get_sequence_number().overflowing_add(1).0;
                    if let Err(err) = reply_ack_ipv4(
                        e.last_sent_acknowledge_number,
                        e.next_sequence_number,
                        e.window_size,
                        &e.segment_info,
                        false,
                    ) {
                        pr_err!("Failed to send ACK: {:?}", err);
                    }
                    e.next_sequence_number = e.next_sequence_number.overflowing_add(1).0;
                }
                if should_delete {
                    e.free_buffer();
                    drop(_lock);
                    cursor.remove_current();
                }
                break;
            }
            cursor.move_next();
        }
    } else if tcp_segment.is_syn_active() {
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
                        let segment_info = TcpSegmentInfo {
                            sender_port,
                            destination_port,
                            packet_info: ipv4_packet_info.clone(),
                            frame_info: frame_info.clone(),
                        };
                        let handler = (e.handler)(segment_info.clone());
                        if let Ok(handler) = handler {
                            e.number_of_active_connection += 1;
                            let sequence_number = 1u32; /* TODO: randomise */
                            let session_entry = TcpSessionInfo {
                                lock: SpinLockFlag::new(),
                                segment_info: segment_info.clone(),
                                window_size: tcp_segment.get_window_size(),
                                buffer_list: LinkedList::new(),
                                next_sequence_number: sequence_number.overflowing_add(1).0,
                                expected_arrive_sequence_number: tcp_segment.get_sequence_number()
                                    + 1,
                                last_sent_acknowledge_number: 0,
                                status: TcpSessionStatus::HalfOpened,
                                handler,
                            };
                            let _session_lock = unsafe { TCP_SESSION_MANAGER.0.lock() };
                            unsafe { TCP_SESSION_MANAGER.1.push_front(session_entry) };
                            drop(_session_lock);
                            if let Err(err) = reply_ack_ipv4(
                                tcp_segment.get_sequence_number() + 1,
                                sequence_number,
                                tcp_segment.get_window_size(),
                                &segment_info,
                                true,
                            ) {
                                pr_err!("Failed to send SYN-ACK: {:?}", err);
                            }
                        } else {
                            pr_err!("Failed to add handler: {:?}", handler.unwrap_err());
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
        let mut cursor = unsafe { TCP_SESSION_MANAGER.1.cursor_front_mut() };
        while let Some(e) = cursor.current() {
            if e.segment_info.get_destination_port() == destination_port
                && e.segment_info.get_sender_port() == sender_port
            {
                match e.status {
                    TcpSessionStatus::HalfOpened => {
                        if tcp_segment.get_sequence_number() == e.expected_arrive_sequence_number
                            && tcp_segment.get_acknowledgement_number() == e.next_sequence_number
                        {
                            pr_debug!("Socket Opened");
                            e.status = TcpSessionStatus::Opened;
                        }
                    }
                    TcpSessionStatus::ClosingOppositeClosed => {
                        let _lock = e.lock.lock();
                        if tcp_segment.get_acknowledgement_number() == e.next_sequence_number {
                            pr_debug!("Closed!!");
                            e.status = TcpSessionStatus::Closed;
                            e.free_buffer();
                            drop(_lock);
                            cursor.remove_current();
                        }
                    }
                    TcpSessionStatus::Closing => {
                        let _lock = e.lock.lock();
                        if tcp_segment.get_acknowledgement_number() == e.next_sequence_number {
                            pr_debug!("Closed!!");
                            e.status = TcpSessionStatus::ClosedOppositeOpened;
                            e.free_buffer();
                            drop(_lock);
                        }
                    }
                    TcpSessionStatus::Opened
                    | TcpSessionStatus::OpenOppositeClosed
                    | TcpSessionStatus::ClosedOppositeOpened => {
                        drop(_session_lock);
                        if tcp_segment.get_header_length()
                            == (data_length.to_usize() - packet_offset)
                        {
                            /* ACK only */
                            //todo!();
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
                    TcpSessionStatus::ClosedOppositeClosing | TcpSessionStatus::Closed => {
                        /* Ignore */
                    }
                }
                break;
            }
            cursor.move_next();
        }
        drop(_session_lock);
        let _ = kfree!(allocated_data_base, data_length);
    } else {
        let _session_lock = unsafe { TCP_SESSION_MANAGER.0.lock() };
        for e in unsafe { TCP_SESSION_MANAGER.1.iter_mut() } {
            if e.segment_info.get_destination_port() == destination_port
                && e.segment_info.get_sender_port() == sender_port
            {
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

pub fn close_session(segment_info: &mut TcpSegmentInfo) {
    let _session_lock = unsafe { TCP_SESSION_MANAGER.0.lock() };
    let mut cursor = unsafe { TCP_SESSION_MANAGER.1.cursor_front_mut() };
    while let Some(e) = cursor.current() {
        if e.segment_info.get_destination_port() == segment_info.get_destination_port()
            && e.segment_info.get_sender_port() == segment_info.get_sender_port()
        {
            let _lock = e.lock.lock();
            match e.status {
                TcpSessionStatus::Closed => {
                    e.free_buffer();
                    drop(_lock);
                    cursor.remove_current();
                    return;
                }
                TcpSessionStatus::Closing
                | TcpSessionStatus::ClosingOppositeClosed
                | TcpSessionStatus::ClosedOppositeOpened => { /* Do nothing */ }
                _ => {
                    if let Err(err) = send_fin_ipv4(
                        e.last_sent_acknowledge_number,
                        e.next_sequence_number,
                        e.window_size,
                        &e.segment_info,
                    ) {
                        pr_err!("Failed to send FIN: {:?}", err);
                        return;
                    }
                    e.next_sequence_number = e.next_sequence_number.overflowing_add(1).0;
                    if e.status == TcpSessionStatus::OpenOppositeClosed {
                        e.status = TcpSessionStatus::ClosingOppositeClosed;
                    } else {
                        e.status = TcpSessionStatus::Closing;
                    }
                }
            }
            break;
        }
        cursor.move_next();
    }
}
