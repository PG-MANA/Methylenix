//!
//! Socket Manager
//!

pub mod socket_system_call;

use super::{InternetType, LinkType, NetworkError, TransportType, ipv4, tcp, udp};

use crate::kernel::collections::ptr_linked_list::{PtrLinkedList, PtrLinkedListNode};
use crate::kernel::collections::ring_buffer::Ringbuffer;
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::data_type::{Address, MSize, VAddress};
use crate::kernel::memory_manager::{kfree, kmalloc};
use crate::kernel::sync::spin_lock::SpinLockFlag;
use crate::kernel::task_manager::wait_queue::WaitQueue;

use core::mem::offset_of;

const DEFAULT_BUFFER_SIZE: usize = 4096;

struct SocketListEntry {
    lock: SpinLockFlag,
    list: PtrLinkedList<Socket>,
}

pub struct SocketManager {
    listening_socket_lock: SpinLockFlag,
    listening_socket: PtrLinkedList<Socket>,
    active_socket: [SocketListEntry; 1 << Self::SOCKET_LIST_ORDER],
}

struct SocketLayerInfo {
    link: LinkType,
    internet: InternetType,
    transport: TransportType,
}

pub struct Socket {
    list: PtrLinkedListNode<Self>,
    lock: SpinLockFlag,
    is_active: bool,
    layer_info: SocketLayerInfo,
    wait_queue: WaitQueue,
    send_ring_buffer: Ringbuffer,
    receive_ring_buffer: Ringbuffer,
    waiting_socket: PtrLinkedList<Self>,
}

impl Default for SocketManager {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> SocketManager {
    const DEFAULT_SOCKET_CLOSE_TIME_OUT_MS: u64 = 10 * 1000;
    /// Self::active_list\[(1 << Self:::SOCKET_LIST_ORDER\]
    const SOCKET_LIST_ORDER: usize = 4;

    pub const fn new() -> Self {
        Self {
            listening_socket_lock: SpinLockFlag::new(),
            listening_socket: PtrLinkedList::new(),
            active_socket: [const {
                SocketListEntry {
                    lock: SpinLockFlag::new(),
                    list: PtrLinkedList::new(),
                }
            }; 1 << Self::SOCKET_LIST_ORDER],
        }
    }

    pub(super) fn create_socket(
        &mut self,
        mut link_type: LinkType,
        internet_type: InternetType,
        transport_type: TransportType,
    ) -> Result<Socket, NetworkError> {
        match &mut link_type {
            LinkType::None => {}
            LinkType::Ethernet(ether) => ether.set_frame_type(match &internet_type {
                InternetType::None => {
                    return Err(NetworkError::InvalidSocket);
                }
                InternetType::Ipv4(_) => ipv4::ETHERNET_TYPE_IPV4,
                InternetType::Ipv6(_) => {
                    unimplemented!()
                }
            }),
        }
        Ok(Socket {
            list: PtrLinkedListNode::new(),
            lock: SpinLockFlag::new(),
            layer_info: SocketLayerInfo {
                link: link_type,
                internet: internet_type,
                transport: transport_type,
            },
            is_active: true,
            wait_queue: WaitQueue::new(),
            send_ring_buffer: Ringbuffer::new(),
            receive_ring_buffer: Ringbuffer::new(),
            waiting_socket: PtrLinkedList::new(),
        })
    }

    pub fn add_socket(&mut self, socket: Socket) -> Result<&'static mut Socket, NetworkError> {
        match kmalloc!(Socket, socket) {
            Ok(s) => {
                s.is_active = true;
                s.list = PtrLinkedListNode::new();
                s.waiting_socket = PtrLinkedList::new();
                let socket_list = &mut self.active_socket[Self::calc_hash_number_of_list(
                    &s.layer_info.internet,
                    &s.layer_info.transport,
                )];
                let _lock = socket_list.lock.lock();
                unsafe { socket_list.list.insert_tail(&mut s.list) };
                drop(_lock);
                Ok(s)
            }
            Err(e) => {
                pr_err!("Failed to create socket: {:?}", e);
                Err(NetworkError::MemoryError(e))
            }
        }
    }

    pub fn add_listening_socket(&'a mut self, socket: &'a mut Socket) -> Result<(), NetworkError> {
        match &mut socket.layer_info.transport {
            TransportType::Tcp(tcp_info) => tcp_info.set_status(tcp::TcpSessionStatus::Listening),
            TransportType::Udp(_) => { /* Do nothing */ }
        }
        socket.is_active = true;
        socket.list = PtrLinkedListNode::new();
        socket.waiting_socket = PtrLinkedList::new();
        let _lock = self.listening_socket_lock.lock();
        unsafe { self.listening_socket.insert_tail(&mut socket.list) };
        drop(_lock);
        Ok(())
    }

    pub fn activate_waiting_socket(
        &mut self,
        socket: &'a mut Socket,
        allow_sleep: bool,
    ) -> Result<&'a mut Socket, NetworkError> {
        let _socket_lock = socket.lock.lock();

        if let Some(waiting_socket) = unsafe {
            socket
                .waiting_socket
                .take_first_entry(offset_of!(Socket, list))
                .map(|e| &mut *e)
        } {
            drop(_socket_lock);
            waiting_socket.list = PtrLinkedListNode::new();
            let socket_list = &mut self.active_socket[Self::calc_hash_number_of_list(
                &waiting_socket.layer_info.internet,
                &waiting_socket.layer_info.transport,
            )];
            let _lock = socket_list.lock.lock();
            unsafe { socket_list.list.insert_tail(&mut waiting_socket.list) };
            drop(_lock);
            /* Send ACK */
            if let TransportType::Tcp(tcp_session) = &mut waiting_socket.layer_info.transport {
                if let Err(err) = tcp::send_tcp_syn_ack_header(
                    tcp_session,
                    &waiting_socket.layer_info.internet,
                    &waiting_socket.layer_info.link,
                ) {
                    pr_err!("Failed to open the session: {:?}", err);
                }
            }
            Ok(waiting_socket)
        } else if allow_sleep {
            drop(_socket_lock);
            if let Err(err) = socket.wait_queue.add_current_thread() {
                pr_err!("Failed to add current thread: {:?}", err);
                return Err(NetworkError::InternalError);
            }
            self.activate_waiting_socket(socket, allow_sleep)
        } else {
            drop(_socket_lock);
            Err(NetworkError::InvalidSocket /* Really OK?*/)
        }
    }

    pub fn read_socket(
        &mut self,
        socket: &mut Socket,
        buffer_address: VAddress,
        buffer_size: MSize,
        allow_sleep: bool,
    ) -> Result<MSize, NetworkError> {
        let _lock = socket.lock.lock();
        let read_size = socket.receive_ring_buffer.read(buffer_address, buffer_size);
        if read_size.is_zero() && allow_sleep {
            drop(_lock);
            if let Err(e) = socket.wait_queue.add_current_thread() {
                pr_err!("Failed to sleep current thread: {:?}", e);
                return Err(NetworkError::InternalError);
            }
            return self.read_socket(socket, buffer_address, buffer_size, allow_sleep);
        }
        Ok(read_size)
    }

    pub fn send_socket(
        &mut self,
        socket: &mut Socket,
        buffer_address: VAddress,
        buffer_size: MSize,
    ) -> Result<MSize, NetworkError> {
        let mut _lock = socket.lock.lock();
        match &mut socket.layer_info.transport {
            TransportType::Tcp(session_info) => match &socket.layer_info.internet {
                InternetType::None => Err(NetworkError::InvalidSocket),
                InternetType::Ipv4(v4) => {
                    let mut current_buffer_address = buffer_address;
                    let mut remaining_size = buffer_size;
                    loop {
                        tcp::send_tcp_ipv4_data(
                            session_info,
                            &mut current_buffer_address,
                            &mut remaining_size,
                            v4,
                            &socket.layer_info.link,
                        )?;
                        if remaining_size.is_zero() {
                            drop(_lock);
                            return Ok(buffer_size);
                        } else {
                            drop(_lock);
                            let _ = socket.wait_queue.add_current_thread();
                            _lock = socket.lock.lock()
                        }
                    }
                }
                InternetType::Ipv6(_) => {
                    unimplemented!();
                }
            },
            TransportType::Udp(u) => match &socket.layer_info.internet {
                InternetType::None => Err(NetworkError::InvalidSocket),
                InternetType::Ipv4(v4) => {
                    let send_buffer_size =
                        MSize::new(udp::UDP_HEADER_SIZE + ipv4::IPV4_DEFAULT_HEADER_SIZE)
                            + buffer_size;
                    let send_buffer = match kmalloc!(send_buffer_size) {
                        Ok(a) => a,
                        Err(e) => {
                            pr_err!("Failed to allocate buffer: {:?}", e);
                            return Err(NetworkError::MemoryError(e));
                        }
                    };

                    let header_buffer = unsafe {
                        &mut *(send_buffer.to_usize()
                            as *mut [u8; udp::UDP_HEADER_SIZE + ipv4::IPV4_DEFAULT_HEADER_SIZE])
                    };
                    udp::create_ipv4_udp_header(
                        header_buffer,
                        unsafe {
                            core::slice::from_raw_parts(
                                buffer_address.to_usize() as *const u8,
                                buffer_size.to_usize(),
                            )
                        },
                        u.get_sender_port(),
                        v4.get_our_address(),
                        u.get_their_port(),
                        v4.get_their_address(),
                        0,
                    )?;

                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            buffer_address.to_usize() as *const u8,
                            (send_buffer.to_usize() + header_buffer.len()) as *mut u8,
                            buffer_size.to_usize(),
                        );
                    }
                    let send_buffer_slice = unsafe {
                        core::slice::from_raw_parts(
                            send_buffer.to_usize() as *const u8,
                            send_buffer_size.to_usize(),
                        )
                    };

                    let result = match &socket.layer_info.link {
                        LinkType::None => Err(NetworkError::InvalidSocket),
                        LinkType::Ethernet(ether) => {
                            drop(_lock); //TODO: clone ether
                            get_kernel_manager_cluster()
                                .network_manager
                                .ethernet_manager
                                .reply_data(ether, send_buffer_slice)
                        }
                    };
                    let _ = kfree!(send_buffer, send_buffer_size);
                    result.map(|_| buffer_size)
                }
                InternetType::Ipv6(_) => {
                    unimplemented!()
                }
            },
        }
    }

    pub fn close_socket(&mut self, socket: &mut Socket) -> Result<(), NetworkError> {
        self._close_socket(socket)
    }

    fn _close_socket(&mut self, socket: &mut Socket) -> Result<(), NetworkError> {
        //assert!(self.lock.is_locked());
        let _socket_lock = socket.lock.lock();
        if !socket.receive_ring_buffer.get_buffer_size().is_zero() {
            let _ = kfree!(
                socket.receive_ring_buffer.get_buffer_address(),
                socket.receive_ring_buffer.get_buffer_size()
            );
            socket.receive_ring_buffer.unset_buffer();
        }
        if !socket.send_ring_buffer.get_buffer_size().is_zero() {
            let _ = kfree!(
                socket.send_ring_buffer.get_buffer_address(),
                socket.send_ring_buffer.get_buffer_size()
            );
            socket.send_ring_buffer.unset_buffer();
        }
        match &mut socket.layer_info.transport {
            TransportType::Tcp(session_info) => {
                socket.is_active = tcp::close_tcp_session(
                    session_info,
                    &mut socket.layer_info.internet,
                    &mut socket.layer_info.link,
                )?;
            }
            TransportType::Udp(_) => {
                socket.is_active = false;
            }
        }
        while let Some(child_socket) = unsafe {
            socket
                .waiting_socket
                .take_first_entry(offset_of!(Socket, list))
                .map(|e| &mut *e)
        } {
            let _child_socket_lock = child_socket.lock.lock();
            if child_socket.is_active {
                drop(_child_socket_lock);
                /* The parent socket will close child_socket */
            } else {
                drop(_child_socket_lock);
                self._close_socket(child_socket)?;
            }
        }

        if socket.is_active {
            if socket.wait_queue.is_empty() {
                bug_on_err!(get_cpu_manager_cluster().local_timer_manager.add_timer(
                    Self::DEFAULT_SOCKET_CLOSE_TIME_OUT_MS,
                    Self::delete_socket,
                    socket as *mut _ as usize,
                ));
            } else {
                bug_on_err!(socket.wait_queue.wakeup_all());
                /* TODO: How to delete socket? */
            }
            drop(_socket_lock);
        } else {
            drop(_socket_lock);
            bug_on_err!(kfree!(socket));
        }
        Ok(())
    }

    fn delete_socket(socket_address: usize) {
        let socket = unsafe { &mut *(socket_address as *mut Socket) };
        let _lock = socket.lock.lock();
        socket.is_active = false;
        drop(_lock);
        bug_on_err!(kfree!(socket));
    }

    /* Data Receive Handlers */

    /* UDP Data Receive Handler */
    pub(super) fn udp_segment_handler(
        &mut self,
        _link_info: LinkType,
        transport_info: InternetType,
        udp_segment_info: udp::UdpSegmentInfo,
        data_buffer: VAddress,
        data_length: MSize,
        payload_base: usize,
    ) {
        let udp_socket_handler = |e: &mut Socket| {
            let _socket_lock = e.lock.lock();
            let payload_size = MSize::new(udp_segment_info.payload_size);
            if e.receive_ring_buffer.get_buffer_size().is_zero() {
                let new_buffer_size = MSize::new(DEFAULT_BUFFER_SIZE);
                match kmalloc!(new_buffer_size) {
                    Ok(a) => {
                        e.receive_ring_buffer.set_new_buffer(a, new_buffer_size);
                    }
                    Err(err) => {
                        pr_err!("Failed to allocate memory: {:?}", err);
                        bug_on_err!(kfree!(data_buffer, data_length));
                        return;
                    }
                }
            }
            let written_size = e
                .receive_ring_buffer
                .write(data_buffer + MSize::new(payload_base), payload_size);
            if payload_size != written_size {
                pr_warn!("Overflowed {} Bytes", payload_size - written_size);
            }
            bug_on_err!(e.wait_queue.wakeup_all());
            drop(_socket_lock);
            bug_on_err!(kfree!(data_buffer, data_length));
        };

        /* Search actually matched socket */
        let socket_list = &mut self.active_socket[Self::calc_hash_number_of_list_tcp_udp(
            &transport_info,
            udp_segment_info.connection_info.get_our_port(),
            udp_segment_info.connection_info.get_their_port(),
        )];
        let _lock = socket_list.lock.lock();
        for e in unsafe { socket_list.list.iter_mut(offset_of!(Socket, list)) } {
            if let TransportType::Udp(udp_info) = &e.layer_info.transport {
                if e.is_active
                    && udp_info.get_our_port()
                        == udp_segment_info.connection_info.get_destination_port()
                    && udp_info.get_their_port()
                        == udp_segment_info.connection_info.get_sender_port()
                {
                    match &e.layer_info.internet {
                        InternetType::None => { /* Check: OK */ }
                        InternetType::Ipv4(ipv4_info) => {
                            let InternetType::Ipv4(arrive_ipv4_info) = &transport_info else {
                                continue;
                            };
                            if (ipv4_info.get_their_address()
                                != arrive_ipv4_info.get_sender_address())
                                || (ipv4_info.get_our_address()
                                    != arrive_ipv4_info.get_destination_address())
                            {
                                continue;
                            }
                        }
                        InternetType::Ipv6(_) => {
                            unimplemented!()
                        }
                    }
                    drop(_lock);
                    udp_socket_handler(e);
                    return;
                }
            }
        }
        drop(_lock);

        /* Search Special Sockets */
        for socket_list in &mut self.active_socket {
            let _lock = socket_list.lock.lock();
            for e in unsafe { socket_list.list.iter_mut(offset_of!(Socket, list)) } {
                if let TransportType::Udp(udp_info) = &e.layer_info.transport {
                    if e.is_active
                        && udp_info.get_our_port()
                            == udp_segment_info.connection_info.get_destination_port()
                        && (udp_info.get_their_port() == udp::UDP_PORT_ANY
                            || udp_info.get_their_port()
                                == udp_segment_info.connection_info.get_sender_port())
                    {
                        match &e.layer_info.internet {
                            InternetType::None => { /* Check: OK */ }
                            InternetType::Ipv4(ipv4_info) => {
                                let InternetType::Ipv4(arrive_ipv4_info) = &transport_info else {
                                    continue;
                                };

                                if ipv4_info.get_their_address() != ipv4::IPV4_ADDRESS_ANY
                                    && ipv4_info.get_their_address() != ipv4::IPV4_BROAD_CAST
                                    && ipv4_info.get_their_address()
                                        != arrive_ipv4_info.get_sender_address()
                                {
                                    continue;
                                }
                                if ipv4_info.get_our_address() != ipv4::IPV4_ADDRESS_ANY
                                    && arrive_ipv4_info.get_destination_address()
                                        != ipv4::IPV4_BROAD_CAST
                                    && ipv4_info.get_our_address()
                                        != arrive_ipv4_info.get_destination_address()
                                {
                                    continue;
                                }
                            }
                            InternetType::Ipv6(_) => {
                                unimplemented!()
                            }
                        }
                        drop(_lock);
                        udp_socket_handler(e);
                        return;
                    }
                }
            }
            drop(_lock);
        }
        pr_debug!("UDP segment will be deleted...");
        bug_on_err!(kfree!(data_buffer, data_length));
    }

    /* TCP Port Open Handler */
    pub(super) fn tcp_port_open_handler(
        &mut self,
        link_info: LinkType,
        internet_info: InternetType,
        tcp_segment_info: &tcp::TcpSegmentInfo,
        new_session_info: tcp::TcpSessionInfo,
    ) -> Result<(), NetworkError> {
        let _lock = self.listening_socket_lock.lock();
        for e in unsafe { self.listening_socket.iter_mut(offset_of!(Socket, list)) } {
            if let TransportType::Tcp(tcp_info) = &e.layer_info.transport {
                if e.is_active
                    && tcp_info.get_status() == tcp::TcpSessionStatus::Listening
                    && tcp_segment_info.get_destination_port() == tcp_info.get_our_port()
                    && (tcp_info.get_their_port() == tcp::TCP_PORT_ANY
                        || tcp_info.get_their_port() == tcp_segment_info.get_sender_port())
                {
                    match &e.layer_info.internet {
                        InternetType::None => { /* Check: OK */ }
                        InternetType::Ipv4(ipv4_info) => {
                            let InternetType::Ipv4(arrive_ipv4_info) = &internet_info else {
                                break;
                            };

                            if ipv4_info.get_their_address() != ipv4::IPV4_ADDRESS_ANY
                                && ipv4_info.get_their_address() != ipv4::IPV4_BROAD_CAST
                                && ipv4_info.get_their_address()
                                    != arrive_ipv4_info.get_sender_address()
                            {
                                break;
                            }
                            if ipv4_info.get_our_address() != ipv4::IPV4_ADDRESS_ANY
                                && arrive_ipv4_info.get_destination_address()
                                    != ipv4::IPV4_BROAD_CAST
                                && ipv4_info.get_our_address()
                                    != arrive_ipv4_info.get_destination_address()
                            {
                                break;
                            }
                        }
                        InternetType::Ipv6(_) => {
                            unimplemented!()
                        }
                    }
                    drop(_lock);

                    let child_socket = kmalloc!(
                        Socket,
                        Socket {
                            list: PtrLinkedListNode::new(),
                            lock: SpinLockFlag::new(),
                            layer_info: SocketLayerInfo {
                                link: link_info,
                                internet: internet_info,
                                transport: TransportType::Tcp(new_session_info),
                            },
                            wait_queue: WaitQueue::new(),
                            send_ring_buffer: Ringbuffer::new(),
                            receive_ring_buffer: Ringbuffer::new(),
                            waiting_socket: PtrLinkedList::new(),
                            is_active: true,
                        }
                    );
                    if let Err(err) = child_socket {
                        pr_err!("Failed to allocate memory: {:?}", err);
                        return Err(NetworkError::MemoryError(err));
                    }
                    let child_socket = child_socket.unwrap();
                    let _socket_lock = e.lock.lock();
                    unsafe { e.waiting_socket.insert_tail(&mut child_socket.list) };
                    bug_on_err!(e.wait_queue.wakeup_all());
                    drop(_socket_lock);
                    return Ok(());
                }
            }
        }
        drop(_lock);
        Err(NetworkError::InvalidAddress)
    }

    pub(super) fn tcp_update_status<F>(
        &mut self,
        _link_info: LinkType,
        internet_info: InternetType,
        tcp_segment_info: &tcp::TcpSegmentInfo,
        update_function: F,
    ) -> Result<(), NetworkError>
    where
        F: FnOnce(&mut tcp::TcpSessionInfo) -> Result<bool /* Active */, NetworkError>,
    {
        let socket_list = &mut self.active_socket[Self::calc_hash_number_of_list_tcp_udp(
            &internet_info,
            tcp_segment_info.get_our_port(),
            tcp_segment_info.get_their_port(),
        )];
        let _lock = socket_list.lock.lock();
        for e in unsafe { socket_list.list.iter_mut(offset_of!(Socket, list)) } {
            if let TransportType::Tcp(tcp_info) = &mut e.layer_info.transport {
                if e.is_active
                    && tcp_segment_info.get_destination_port() == tcp_info.get_our_port()
                    && tcp_info.get_their_port() == tcp_segment_info.get_sender_port()
                {
                    match &e.layer_info.internet {
                        InternetType::None => { /* Check: OK */ }
                        InternetType::Ipv4(ipv4_info) => {
                            let InternetType::Ipv4(arrive_ipv4_info) = &internet_info else {
                                break;
                            };
                            if ipv4_info.get_their_address() != ipv4::IPV4_ADDRESS_ANY
                                && ipv4_info.get_their_address() != ipv4::IPV4_BROAD_CAST
                                && ipv4_info.get_their_address()
                                    != arrive_ipv4_info.get_sender_address()
                            {
                                break;
                            }
                            if ipv4_info.get_our_address() != ipv4::IPV4_ADDRESS_ANY
                                && arrive_ipv4_info.get_destination_address()
                                    != ipv4::IPV4_BROAD_CAST
                                && ipv4_info.get_our_address()
                                    != arrive_ipv4_info.get_destination_address()
                            {
                                break;
                            }
                        }
                        InternetType::Ipv6(_) => {
                            unimplemented!()
                        }
                    }
                    drop(_lock);
                    let _socket_lock = e.lock.lock();
                    let result = update_function(tcp_info).map(|active| e.is_active = active);
                    bug_on_err!(e.wait_queue.wakeup_all());
                    drop(_socket_lock);
                    return result;
                }
            }
        }
        drop(_lock);
        pr_debug!("Failed to search valid socket.");
        Err(NetworkError::InvalidAddress)
    }

    pub(super) fn tcp_data_receive_handler<F>(
        &mut self,
        _link_info: LinkType,
        internet_info: InternetType,
        tcp_segment_info: &tcp::TcpSegmentInfo,
        process_function: F,
    ) -> Result<(), NetworkError>
    where
        F: FnOnce(&mut tcp::TcpSessionInfo, &mut Ringbuffer) -> Result<(), NetworkError>,
    {
        let socket_list = &mut self.active_socket[Self::calc_hash_number_of_list_tcp_udp(
            &internet_info,
            tcp_segment_info.get_our_port(),
            tcp_segment_info.get_their_port(),
        )];
        let _lock = socket_list.lock.lock();
        for e in unsafe { socket_list.list.iter_mut(offset_of!(Socket, list)) } {
            if let TransportType::Tcp(tcp_info) = &mut e.layer_info.transport {
                if e.is_active
                    && tcp_info.get_status() == tcp::TcpSessionStatus::Opened
                    && tcp_segment_info.get_destination_port() == tcp_info.get_our_port()
                    && tcp_info.get_their_port() == tcp_segment_info.get_sender_port()
                {
                    match &e.layer_info.internet {
                        InternetType::None => { /* Check: OK */ }
                        InternetType::Ipv4(ipv4_info) => {
                            let InternetType::Ipv4(arrive_ipv4_info) = &internet_info else {
                                break;
                            };
                            if ipv4_info.get_their_address() != ipv4::IPV4_ADDRESS_ANY
                                && ipv4_info.get_their_address() != ipv4::IPV4_BROAD_CAST
                                && ipv4_info.get_their_address()
                                    != arrive_ipv4_info.get_sender_address()
                            {
                                break;
                            }
                            if ipv4_info.get_our_address() != ipv4::IPV4_ADDRESS_ANY
                                && arrive_ipv4_info.get_destination_address()
                                    != ipv4::IPV4_BROAD_CAST
                                && ipv4_info.get_our_address()
                                    != arrive_ipv4_info.get_destination_address()
                            {
                                break;
                            }
                        }
                        InternetType::Ipv6(_) => {
                            unimplemented!()
                        }
                    }
                    drop(_lock);
                    let _socket_lock = e.lock.lock();
                    if e.receive_ring_buffer.get_buffer_size().is_zero() {
                        let new_buffer_size = MSize::new(DEFAULT_BUFFER_SIZE);
                        match kmalloc!(new_buffer_size) {
                            Ok(a) => {
                                e.receive_ring_buffer.set_new_buffer(a, new_buffer_size);
                            }
                            Err(err) => {
                                drop(_socket_lock);
                                pr_err!("Failed to allocate memory: {:?}", err);
                                return Err(NetworkError::MemoryError(err));
                            }
                        }
                    }
                    let result = process_function(tcp_info, &mut e.receive_ring_buffer);
                    bug_on_err!(e.wait_queue.wakeup_all());
                    drop(_socket_lock);

                    return result;
                }
            }
        }
        drop(_lock);
        pr_debug!("Failed to search valid socket.");
        Err(NetworkError::InvalidAddress)
    }

    fn _calc_hash_number_of_list_ip(
        mut seed: usize,
        our_address: &[u8],
        their_address: &[u8],
    ) -> usize {
        for e in our_address {
            seed += *e as usize;
        }
        for e in their_address {
            seed += *e as usize;
        }
        seed
    }

    fn _calc_hash_number_of_list_tp(seed: usize, our_port: u16, their_port: u16) -> usize {
        seed * (our_port as usize + their_port as usize)
    }

    fn _calc_hash_number_of_list_finalize(value: usize) -> usize {
        value & ((1 << Self::SOCKET_LIST_ORDER) - 1)
    }

    fn calc_hash_number_of_list(
        internet_type: &InternetType,
        transport_type: &TransportType,
    ) -> usize {
        let mut hash: usize = 0;
        match &internet_type {
            InternetType::None => { /* Do nothing */ }
            InternetType::Ipv4(v4) => {
                hash = Self::_calc_hash_number_of_list_ip(
                    hash,
                    &v4.get_our_address().to_ne_bytes(),
                    &v4.get_their_address().to_ne_bytes(),
                );
            }
            InternetType::Ipv6(_v6) => {
                unimplemented!()
            }
        }
        match &transport_type {
            TransportType::Tcp(t) => {
                hash =
                    Self::_calc_hash_number_of_list_tp(hash, t.get_our_port(), t.get_their_port());
            }
            TransportType::Udp(t) => {
                hash =
                    Self::_calc_hash_number_of_list_tp(hash, t.get_our_port(), t.get_their_port());
            }
        }
        Self::_calc_hash_number_of_list_finalize(hash)
    }

    fn calc_hash_number_of_list_tcp_udp(
        internet_type: &InternetType,
        our_port: u16,
        their_port: u16,
    ) -> usize {
        let mut hash: usize = 0;
        match &internet_type {
            InternetType::None => { /* Do nothing */ }
            InternetType::Ipv4(v4) => {
                hash = Self::_calc_hash_number_of_list_ip(
                    hash,
                    &v4.get_our_address().to_ne_bytes(),
                    &v4.get_their_address().to_ne_bytes(),
                );
            }
            InternetType::Ipv6(_v6) => {
                unimplemented!()
            }
        }
        hash = Self::_calc_hash_number_of_list_tp(hash, our_port, their_port);
        Self::_calc_hash_number_of_list_finalize(hash)
    }
}
