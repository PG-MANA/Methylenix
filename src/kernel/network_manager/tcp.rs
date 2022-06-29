//!
//! TCP
//!

use super::{ipv4, AddressPrinter, InternetType, LinkType, NetworkError};

use crate::kernel::collections::ring_buffer::Ringbuffer;
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::data_type::{Address, MOffset, MSize, VAddress};

use crate::{kfree, kmalloc};

use crate::kernel::collections::ptr_linked_list::{PtrLinkedList, PtrLinkedListNode};
use alloc::collections::LinkedList;
use core::ptr::copy_nonoverlapping;

pub const IPV4_PROTOCOL_TCP: u8 = 0x06;
pub const MAX_SEGMENT_SIZE: usize = 1460;
pub const MAX_TRANSMISSION_UNIT: usize = 1500;
pub const TCP_DEFAULT_HEADER_SIZE: usize = core::mem::size_of::<DefaultTcpSegment>();
pub const TCP_PORT_ANY: u16 = 0;

struct TcpReceiveDataBuffer {
    allocated_data_base: VAddress,
    data_length: MSize,
    payload_offset: MOffset,
    payload_size: MSize,
    sequence_number: u32,
}

struct TcpSendDataBufferHeader {
    list: PtrLinkedListNode<Self>,
    buffer_length: MSize, /* Including this header */
    sequence_number: u32,
    _padding: u32,
}

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub enum TcpSessionStatus {
    Listening,
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

pub struct TcpSessionInfo {
    status: TcpSessionStatus,
    our_port: u16,
    their_port: u16,
    window_size: u16,
    expected_arrival_sequence_number: u32,
    next_sequence_number: u32,
    last_sent_acknowledge_number: u32,
    receive_buffer_list: LinkedList<TcpReceiveDataBuffer>,
    send_buffer_list: PtrLinkedList<TcpSendDataBufferHeader>,
}

impl TcpSessionInfo {
    pub fn new(our_port: u16, their_port: u16) -> Self {
        Self {
            status: TcpSessionStatus::Closed,
            our_port,
            their_port,
            window_size: 0,
            expected_arrival_sequence_number: 0,
            next_sequence_number: 0,
            last_sent_acknowledge_number: 0,
            receive_buffer_list: LinkedList::new(),
            send_buffer_list: PtrLinkedList::new(),
        }
    }

    pub fn get_our_port(&self) -> u16 {
        self.our_port
    }

    pub fn get_their_port(&self) -> u16 {
        self.their_port
    }

    pub fn get_status(&self) -> TcpSessionStatus {
        self.status
    }

    pub fn set_status(&mut self, status: TcpSessionStatus) {
        self.status = status;
    }

    fn free_buffer(&mut self) {
        while let Some(e) = self.receive_buffer_list.pop_front() {
            if !e.data_length.is_zero() {
                let _ = kfree!(e.allocated_data_base, e.data_length);
            }
        }
        while let Some(e) = unsafe {
            self.send_buffer_list
                .take_first_entry(offset_of!(TcpSendDataBufferHeader, list))
        } {
            if !e.buffer_length.is_zero() {
                let _ = kfree!(VAddress::new(e as *const _ as usize), e.buffer_length);
            }
        }
    }
}

pub struct TcpSegmentInfo {
    sender_port: u16,
    destination_port: u16,
    sequence_number: u32,
    acknowledgement_number: u32,
    window_size: u16,
}

impl TcpSegmentInfo {
    pub const fn get_sender_port(&self) -> u16 {
        self.sender_port
    }

    pub const fn get_destination_port(&self) -> u16 {
        self.destination_port
    }

    pub const fn get_their_port(&self) -> u16 {
        self.get_sender_port()
    }

    pub const fn get_our_port(&self) -> u16 {
        self.get_destination_port()
    }

    pub const fn get_window_size(&self) -> u16 {
        self.window_size
    }

    pub const fn get_sequence_number(&self) -> u32 {
        self.sequence_number
    }

    pub const fn get_acknowledgement_number(&self) -> u32 {
        self.acknowledgement_number
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

pub(super) fn create_ipv4_tcp_header(
    buffer: &mut [u8; TCP_DEFAULT_HEADER_SIZE + ipv4::IPV4_DEFAULT_HEADER_SIZE],
    segment_info: &TcpSegmentInfo,
    data: &[u8],
    fin: bool,
    syn: bool,
    ack: bool,
    psh: bool,
    ipv4_packet_id: u16,
    sender_ipv4_address: u32,
    destination_ipv4_address: u32,
) -> Result<(), NetworkError> {
    *buffer = [0u8; TCP_DEFAULT_HEADER_SIZE + ipv4::IPV4_DEFAULT_HEADER_SIZE];
    let (ipv4_header, tcp_header) = buffer.split_at_mut(ipv4::IPV4_DEFAULT_HEADER_SIZE);
    let tcp_segment = DefaultTcpSegment::from_buffer(tcp_header);

    tcp_segment.set_header_length(TCP_DEFAULT_HEADER_SIZE as u8);
    if fin {
        tcp_segment.set_fin_active();
    }
    if syn {
        tcp_segment.set_syn_active();
    }
    if ack {
        tcp_segment.set_ack_active();
    }
    if psh {
        tcp_segment.set_psh_active();
    }
    tcp_segment.set_destination_port(segment_info.get_destination_port());
    tcp_segment.set_sender_port(segment_info.get_sender_port());
    tcp_segment.set_acknowledgement_number(segment_info.get_acknowledgement_number());
    tcp_segment.set_sequence_number(segment_info.get_sequence_number());
    tcp_segment.set_window_size(segment_info.get_window_size());

    tcp_segment.set_checksum_ipv4(
        sender_ipv4_address,
        destination_ipv4_address,
        TCP_DEFAULT_HEADER_SIZE as u16,
        data,
    );

    ipv4::create_default_ipv4_header(
        ipv4_header,
        TCP_DEFAULT_HEADER_SIZE as usize + data.len(),
        ipv4_packet_id,
        ipv4::get_default_ttl(),
        IPV4_PROTOCOL_TCP,
        sender_ipv4_address,
        destination_ipv4_address,
    )?;

    Ok(())
}

pub(super) fn send_ipv4_tcp_header(
    segment_info: &TcpSegmentInfo,
    fin: bool,
    syn: bool,
    ack: bool,
    ipv4_packet_id: u16,
    sender_ipv4_address: u32,
    destination_ipv4_address: u32,
    link_info: &LinkType,
) -> Result<(), NetworkError> {
    let mut header = [0u8; TCP_DEFAULT_HEADER_SIZE + ipv4::IPV4_DEFAULT_HEADER_SIZE];
    create_ipv4_tcp_header(
        &mut header,
        segment_info,
        &[],
        fin,
        syn,
        ack,
        false,
        ipv4_packet_id,
        sender_ipv4_address,
        destination_ipv4_address,
    )?;

    match link_info {
        LinkType::None => Err(NetworkError::InvalidDevice),
        LinkType::Ethernet(ether) => get_kernel_manager_cluster()
            .network_manager
            .ethernet_manager
            .reply_data(ether, &header),
    }
}

pub(super) fn send_tcp_ipv4_data(
    session_info: &mut TcpSessionInfo,
    data_address: VAddress,
    data_size: MSize,
    ipv4_info: &ipv4::Ipv4ConnectionInfo,
    link_info: &LinkType,
) -> Result<(), NetworkError> {
    if session_info.get_status() != TcpSessionStatus::Opened
        && session_info.get_status() != TcpSessionStatus::OpenOppositeClosed
    {
        pr_err!("Invalid Socket: {:?}", session_info.get_status());
        return Err(NetworkError::InvalidSocket);
    }
    let mut remaining_size = data_size;

    while !remaining_size.is_zero() {
        let send_size = remaining_size.min(MSize::new(MAX_SEGMENT_SIZE));
        pr_debug!("Send Size: {}", send_size);
        const TCP_SEND_DATA_HEADER_SIZE: MSize =
            MSize::new(core::mem::size_of::<TcpSendDataBufferHeader>());
        const PACKET_HEADER_SIZE: MSize =
            MSize::new(TCP_DEFAULT_HEADER_SIZE + ipv4::IPV4_DEFAULT_HEADER_SIZE);

        let allocate_size = send_size + PACKET_HEADER_SIZE + TCP_SEND_DATA_HEADER_SIZE;
        let tcp_send_data_entry = match kmalloc!(allocate_size) {
            Ok(a) => a,
            Err(err) => {
                pr_err!("Failed to allocate memory: {:?}", err);
                return Err(NetworkError::MemoryError(err));
            }
        };
        let header =
            unsafe { &mut *(tcp_send_data_entry.to_usize() as *mut TcpSendDataBufferHeader) };
        init_struct!(
            *header,
            TcpSendDataBufferHeader {
                list: PtrLinkedListNode::new(),
                buffer_length: allocate_size,
                sequence_number: session_info.next_sequence_number,
                _padding: 0
            }
        );
        let payload_base = tcp_send_data_entry + TCP_SEND_DATA_HEADER_SIZE + PACKET_HEADER_SIZE;
        unsafe {
            copy_nonoverlapping(
                (data_address + (data_size - remaining_size)).to_usize() as *const u8,
                payload_base.to_usize() as *mut u8,
                send_size.to_usize(),
            )
        };
        let segment_info = TcpSegmentInfo {
            sender_port: session_info.get_our_port(),
            destination_port: session_info.get_their_port(),
            sequence_number: session_info.next_sequence_number,
            acknowledgement_number: session_info.last_sent_acknowledge_number,
            window_size: session_info.window_size,
        };
        if let Err(e) = create_ipv4_tcp_header(
            unsafe {
                &mut *((tcp_send_data_entry + TCP_SEND_DATA_HEADER_SIZE).to_usize()
                    as *mut [u8; PACKET_HEADER_SIZE.to_usize()])
            },
            &segment_info,
            unsafe {
                core::slice::from_raw_parts(
                    payload_base.to_usize() as *const u8,
                    send_size.to_usize(),
                )
            },
            false,
            false,
            true,
            true,
            0,
            ipv4_info.get_our_address(),
            ipv4_info.get_their_address(),
        ) {
            pr_err!("Failed to create header: {:?}", e);
            let _ = kfree!(tcp_send_data_entry, allocate_size);
            return Err(e);
        }
        match link_info {
            LinkType::None => {
                pr_err!("Invalid Socket");
                let _ = kfree!(tcp_send_data_entry, allocate_size);
                return Err(NetworkError::InvalidSocket);
            }
            LinkType::Ethernet(ether) => {
                if let Err(err) = get_kernel_manager_cluster()
                    .network_manager
                    .ethernet_manager
                    .reply_data(ether, unsafe {
                        core::slice::from_raw_parts(
                            (tcp_send_data_entry + TCP_SEND_DATA_HEADER_SIZE).to_usize()
                                as *const u8,
                            (send_size + PACKET_HEADER_SIZE).to_usize(),
                        )
                    })
                {
                    pr_err!("Failed to send data: {:?}", err);
                    let _ = kfree!(tcp_send_data_entry, allocate_size);
                    return Err(err);
                }
            }
        }
        session_info.next_sequence_number = session_info
            .next_sequence_number
            .overflowing_add(send_size.to_usize() as u32)
            .0;
        session_info.send_buffer_list.insert_tail(&mut header.list);
        remaining_size -= send_size;
    }
    return Ok(());
}

pub(super) fn ipv4_tcp_ack_handler(
    session_info: &mut TcpSessionInfo,
    segment_info: &TcpSegmentInfo,
) -> Result<bool, NetworkError> {
    if session_info.get_status() == TcpSessionStatus::HalfOpened {
        if session_info.next_sequence_number == segment_info.get_acknowledgement_number() {
            pr_debug!("Opened");
            session_info.set_status(TcpSessionStatus::Opened);
            return Ok(true);
        }
    } else if session_info.get_status() == TcpSessionStatus::ClosingOppositeClosed {
        if session_info.next_sequence_number == segment_info.get_acknowledgement_number() {
            pr_debug!("Closed");
            session_info.set_status(TcpSessionStatus::Closed);
            session_info.free_buffer();
        }
        return Ok(false);
    } else if session_info.get_status() == TcpSessionStatus::Closing {
        if session_info.next_sequence_number == segment_info.get_acknowledgement_number() {
            pr_debug!("Closed, but opposite still opened");
            session_info.set_status(TcpSessionStatus::ClosedOppositeOpened);
            session_info.free_buffer();
        }
        return Ok(true);
    }
    for entry in unsafe {
        session_info
            .send_buffer_list
            .iter_mut(offset_of!(TcpSendDataBufferHeader, list))
    } {
        if segment_info.get_acknowledgement_number() == entry.sequence_number {
            pr_debug!("Ack: {:#X}", segment_info.get_acknowledgement_number());
            session_info.send_buffer_list.remove(&mut entry.list);
            let _ = kfree!(
                VAddress::new(entry as *const _ as usize),
                entry.buffer_length
            );
            return Ok(true);
        }
    }
    return Ok(true);
}

pub(super) fn tcp_ipv4_segment_handler(
    allocated_data_base: VAddress,
    data_length: MSize,
    segment_offset: usize,
    segment_size: usize,
    link_info: LinkType,
    ipv4_packet_info: ipv4::Ipv4ConnectionInfo,
) {
    if segment_size < TCP_DEFAULT_HEADER_SIZE {
        pr_err!("Invalid TCP header size");
        let _ = kfree!(allocated_data_base, data_length);
        return;
    }
    let tcp_segment = DefaultTcpSegment::from_buffer(unsafe {
        &mut *((allocated_data_base.to_usize() + segment_offset)
            as *mut [u8; TCP_DEFAULT_HEADER_SIZE])
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
        segment_size,
        tcp_segment.get_header_length(),
        tcp_segment.is_ack_active(),
        tcp_segment.is_syn_active(),
        tcp_segment.is_rst_active(),
        tcp_segment.is_fin_active()
    );

    let segment_info = TcpSegmentInfo {
        sender_port: tcp_segment.get_sender_port(),
        destination_port: tcp_segment.get_destination_port(),
        sequence_number: tcp_segment.get_sequence_number(),
        acknowledgement_number: tcp_segment.get_acknowledgement_number(),
        window_size: tcp_segment.get_window_size(),
    };

    if tcp_segment.is_syn_active() && tcp_segment.is_ack_active() {
        /* TCP SYN+ACK */
        pr_debug!("TCP SYN ACK is not supported yet.");
        let _ = kfree!(allocated_data_base, data_length);
    } else if tcp_segment.is_syn_active() && !tcp_segment.is_ack_active() {
        /* TCP SYN */
        let seed = get_cpu_manager_cluster()
            .local_timer_manager
            .get_monotonic_clock_ns();
        let sequence_number = ((seed >> 32) ^ (seed & u32::MAX as u64)) as u32;

        let new_session = TcpSessionInfo {
            status: TcpSessionStatus::HalfOpened,
            our_port: segment_info.get_our_port(),
            their_port: segment_info.get_their_port(),
            window_size: segment_info.get_window_size(),
            expected_arrival_sequence_number: segment_info
                .get_sequence_number()
                .overflowing_add(1)
                .0,
            next_sequence_number: sequence_number.overflowing_add(1).0,
            last_sent_acknowledge_number: segment_info.get_sequence_number().overflowing_add(1).0,
            receive_buffer_list: LinkedList::new(),
            send_buffer_list: PtrLinkedList::new(),
        };

        let result = get_kernel_manager_cluster()
            .network_manager
            .get_socket_manager()
            .tcp_port_open_handler(
                link_info.clone(),
                InternetType::Ipv4(ipv4_packet_info.clone()),
                &segment_info,
                new_session,
            );

        if result.is_ok() {
            let reply_segment_info = TcpSegmentInfo {
                sender_port: segment_info.get_destination_port(),
                destination_port: segment_info.get_sender_port(),
                sequence_number,
                acknowledgement_number: segment_info.get_sequence_number().overflowing_add(1).0,
                window_size: segment_info.get_window_size(),
            };
            if let Err(e) = send_ipv4_tcp_header(
                &reply_segment_info,
                false,
                true,
                true,
                0,
                ipv4_packet_info.get_destination_address(),
                ipv4_packet_info.get_sender_address(),
                &link_info,
            ) {
                pr_err!("Failed to send SYN+ACK: {:?}", e);
            }
        }
        let _ = kfree!(allocated_data_base, data_length);
    } else if tcp_segment.is_fin_active() {
        /* TCP FIN */
        if let Err(err) = get_kernel_manager_cluster()
            .network_manager
            .get_socket_manager()
            .tcp_update_status(
                link_info.clone(),
                InternetType::Ipv4(ipv4_packet_info.clone()),
                &segment_info,
                |session_info| {
                    ipv4_tcp_fin_handler(
                        session_info,
                        &segment_info,
                        &link_info,
                        &ipv4_packet_info,
                        tcp_segment.is_ack_active(),
                    )
                },
            )
        {
            pr_err!("Failed to process TCP FIN: {:?}", err);
        }
        let _ = kfree!(allocated_data_base, data_length);
    } else if tcp_segment.is_ack_active() && segment_size == tcp_segment.get_header_length() {
        /* ACK Only */
        pr_debug!(
            "TCP ACK: {{Seq: {:#X}, ACK: {:#X}}}",
            tcp_segment.get_sequence_number(),
            tcp_segment.get_acknowledgement_number()
        );
        if let Err(err) = get_kernel_manager_cluster()
            .network_manager
            .get_socket_manager()
            .tcp_update_status(
                link_info,
                InternetType::Ipv4(ipv4_packet_info),
                &segment_info,
                |session_info| ipv4_tcp_ack_handler(session_info, &segment_info),
            )
        {
            pr_err!("Failed to process the ACK: {:?}", err);
        }
        let _ = kfree!(allocated_data_base, data_length);
    } else {
        /* ACK and Data or only Data */
        let mut should_free_buffer = true;

        if let Err(err) = get_kernel_manager_cluster()
            .network_manager
            .get_socket_manager()
            .tcp_data_receive_handler(
                link_info.clone(),
                InternetType::Ipv4(ipv4_packet_info.clone()),
                &segment_info,
                |session_info, write_buffer| {
                    if tcp_segment.is_ack_active() {
                        ipv4_tcp_ack_handler(session_info, &segment_info)?;
                    }
                    ipv4_tcp_data_handler(
                        session_info,
                        &segment_info,
                        write_buffer,
                        allocated_data_base,
                        data_length,
                        MOffset::new(segment_offset + tcp_segment.get_header_length()),
                        MSize::new(segment_size - tcp_segment.get_header_length()),
                        &link_info,
                        &ipv4_packet_info,
                    )
                    .and_then(|s| Ok(should_free_buffer = s))
                },
            )
        {
            pr_err!("Failed to process TCP FIN: {:?}", err);
        }
        if should_free_buffer {
            let _ = kfree!(allocated_data_base, data_length);
        }
    }
}

fn ipv4_tcp_fin_handler(
    session_info: &mut TcpSessionInfo,
    segment_info: &TcpSegmentInfo,
    link_info: &LinkType,
    ipv4_packet_info: &ipv4::Ipv4ConnectionInfo,
    is_ack_active: bool,
) -> Result<bool /* Socket Active */, NetworkError> {
    let mut is_socket_active = true;
    let mut should_send_ack = true;

    match session_info.get_status() {
        TcpSessionStatus::Closed => {
            pr_debug!("Socket Closed");
            is_socket_active = false;
            should_send_ack = false;
        }
        TcpSessionStatus::Closing => {
            pr_debug!(
                "Arrived Ack: {:#X}, Expected Ack: {:#X}",
                segment_info.get_acknowledgement_number(),
                session_info.next_sequence_number
            );
            if is_ack_active {
                if segment_info.get_acknowledgement_number() == session_info.next_sequence_number {
                    session_info.set_status(TcpSessionStatus::ClosedOppositeClosing);
                } else {
                    should_send_ack = false;
                }
            } else {
                session_info.set_status(TcpSessionStatus::ClosingOppositeClosed);
            }
        }
        TcpSessionStatus::HalfOpened => {
            session_info.set_status(TcpSessionStatus::ClosingOppositeClosed);
        }
        TcpSessionStatus::Opened | TcpSessionStatus::OpenOppositeClosed => {
            session_info.set_status(TcpSessionStatus::OpenOppositeClosed);
        }
        TcpSessionStatus::ClosingOppositeClosed | TcpSessionStatus::ClosedOppositeClosing => {
            /* Resend ACK */
        }
        TcpSessionStatus::ClosedOppositeOpened => {
            pr_debug!("Opposite closing");
            session_info.set_status(TcpSessionStatus::ClosedOppositeClosing);
        }
        TcpSessionStatus::Listening => {
            pr_err!("Closing listening socket is invalid!");
            return Err(NetworkError::InvalidSocket);
        }
    }
    if should_send_ack {
        session_info.last_sent_acknowledge_number =
            segment_info.get_sequence_number().overflowing_add(1).0;
        let reply_segment_info = TcpSegmentInfo {
            sender_port: segment_info.get_destination_port(),
            destination_port: segment_info.get_sender_port(),
            sequence_number: session_info.next_sequence_number,
            acknowledgement_number: session_info.last_sent_acknowledge_number,
            window_size: segment_info.get_window_size(),
        };
        if let Err(err) = send_ipv4_tcp_header(
            &reply_segment_info,
            false,
            false,
            true,
            0,
            ipv4_packet_info.get_destination_address(),
            ipv4_packet_info.get_sender_address(),
            link_info,
        ) {
            pr_err!("Failed to send ACK: {:?}", err);
            Err(err)
        } else {
            session_info.next_sequence_number =
                session_info.next_sequence_number.overflowing_add(1).0;
            Ok(is_socket_active)
        }
    } else {
        Ok(is_socket_active)
    }
}

fn ipv4_tcp_data_handler(
    session_info: &mut TcpSessionInfo,
    segment_info: &TcpSegmentInfo,
    write_buffer: &mut Ringbuffer,
    allocated_data_base: VAddress,
    data_length: MSize,
    payload_offset: MOffset,
    payload_size: MSize,
    link_info: &LinkType,
    ipv4_packet_info: &ipv4::Ipv4ConnectionInfo,
) -> Result<bool /* Is allocated_data_base freed? */, NetworkError> {
    let mut should_free_data = true;

    if segment_info.get_sequence_number() == session_info.expected_arrival_sequence_number {
        /* Arrived sequentially */

        let written_size = write_buffer.write(allocated_data_base + payload_offset, payload_size);
        if written_size < payload_size {
            pr_err!("{} Bytes are overflowed.", payload_size - written_size);
            /* TODO: Rollback */
            return Err(NetworkError::DataOverflowed);
        }
        if session_info.receive_buffer_list.len() != 0 {
            /* The packets are not arrived sequentially */
            let mut next_sequence_number = segment_info
                .sequence_number
                .overflowing_add(payload_size.to_usize() as u32)
                .0;
            'outer_loop: loop {
                let mut cursor = session_info.receive_buffer_list.cursor_front_mut();
                while let Some(buffer_entry) = cursor.current() {
                    if buffer_entry.sequence_number == next_sequence_number {
                        next_sequence_number = next_sequence_number
                            .overflowing_add(payload_size.to_usize() as u32)
                            .0;
                        let written_size = write_buffer.write(
                            buffer_entry.allocated_data_base + buffer_entry.payload_offset,
                            buffer_entry.payload_size,
                        );
                        if written_size < payload_size {
                            pr_err!("{} Bytes are overflowed.", payload_size - written_size);
                            /* TODO: Rollback */
                            return Err(NetworkError::DataOverflowed);
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
        session_info.expected_arrival_sequence_number = segment_info
            .get_sequence_number()
            .overflowing_add(payload_size.to_usize() as u32)
            .0;
    } else {
        /* Arrived the data after of the next data */
        session_info
            .receive_buffer_list
            .push_back(TcpReceiveDataBuffer {
                allocated_data_base,
                data_length,
                payload_offset,
                payload_size,
                sequence_number: segment_info.get_sequence_number(),
            });
        should_free_data = false;
    }
    session_info.last_sent_acknowledge_number = segment_info
        .get_sequence_number()
        .overflowing_add(payload_size.to_usize() as u32)
        .0;

    /* Send ACK */
    let reply_segment_info = TcpSegmentInfo {
        sender_port: segment_info.get_destination_port(),
        destination_port: segment_info.get_sender_port(),
        sequence_number: session_info.next_sequence_number,
        acknowledgement_number: session_info.last_sent_acknowledge_number,
        window_size: segment_info.get_window_size(),
    };
    if let Err(e) = send_ipv4_tcp_header(
        &reply_segment_info,
        false,
        false,
        true,
        0,
        ipv4_packet_info.get_destination_address(),
        ipv4_packet_info.get_sender_address(),
        &link_info,
    ) {
        pr_err!("Failed to send SYN+ACK: {:?}", e);
    }

    Ok(should_free_data)
}

pub(super) fn close_tcp_session(
    session_info: &mut TcpSessionInfo,
    internet_info: &InternetType,
    link_info: &LinkType,
) -> Result<bool /* Is Socket Active */, NetworkError> {
    match session_info.get_status() {
        TcpSessionStatus::Listening => Ok(false),
        TcpSessionStatus::Closed => {
            session_info.free_buffer();
            Ok(false)
        }
        TcpSessionStatus::Closing
        | TcpSessionStatus::ClosingOppositeClosed
        | TcpSessionStatus::ClosedOppositeOpened => {
            /* Do nothing */
            Ok(true)
        }
        _ => {
            let segment_info = TcpSegmentInfo {
                sender_port: session_info.get_our_port(),
                destination_port: session_info.get_their_port(),
                sequence_number: session_info.next_sequence_number,
                acknowledgement_number: session_info.last_sent_acknowledge_number,
                window_size: session_info.window_size,
            };
            match internet_info {
                InternetType::None => {
                    return Err(NetworkError::InvalidSocket);
                }
                InternetType::Ipv4(v4_info) => {
                    if let Err(e) = send_ipv4_tcp_header(
                        &segment_info,
                        true,
                        false,
                        true,
                        0,
                        v4_info.get_our_address(),
                        v4_info.get_their_address(),
                        &link_info,
                    ) {
                        pr_err!("Failed to send FIN: {:?}", e);
                        return Err(e);
                    }
                }
                InternetType::Ipv6(_) => {
                    unimplemented!()
                }
            }
            session_info.next_sequence_number =
                session_info.next_sequence_number.overflowing_add(1).0;
            if session_info.get_status() == TcpSessionStatus::OpenOppositeClosed {
                session_info.set_status(TcpSessionStatus::ClosingOppositeClosed);
            } else {
                session_info.set_status(TcpSessionStatus::Closing);
            }
            Ok(true)
        }
    }
}
