//!
//! Socket Manager
//!

use super::{ipv4, tcp, udp, InternetType, LinkType, NetworkError, TransportType};

use crate::kernel::collections::ptr_linked_list::{PtrLinkedList, PtrLinkedListNode};
use crate::kernel::collections::ring_buffer::Ringbuffer;
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{Address, MSize, VAddress};
use crate::kernel::sync::spin_lock::{IrqSaveSpinLockFlag, SpinLockFlag};
use crate::kernel::task_manager::wait_queue::WaitQueue;

use crate::{kfree, kmalloc};

pub mod socket_system_call;

const DEFAULT_BUFFER_SIZE: usize = 4096;

pub struct SocketManager {
    lock: IrqSaveSpinLockFlag,
    active_socket: PtrLinkedList<Socket>,
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

impl SocketManager {
    pub fn new() -> Self {
        Self {
            lock: IrqSaveSpinLockFlag::new(),
            active_socket: PtrLinkedList::new(),
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

    pub fn add_socket(
        &'static mut self,
        socket: Socket,
    ) -> Result<&'static mut Socket, NetworkError> {
        match kmalloc!(Socket, socket) {
            Ok(s) => {
                let _lock = self.lock.lock();
                s.is_active = true;
                self.active_socket.insert_tail(&mut s.list);
                drop(_lock);
                Ok(s)
            }
            Err(e) => {
                pr_err!("Failed to create socket: {:?}", e);
                Err(NetworkError::MemoryError(e))
            }
        }
    }

    pub fn add_listening_socket(
        &'static mut self,
        socket: &'static mut Socket,
    ) -> Result<(), NetworkError> {
        match &mut socket.layer_info.transport {
            TransportType::Tcp(tcp_info) => tcp_info.set_status(tcp::TcpSessionStatus::Listening),
            TransportType::Udp(_) => { /* Do nothing */ }
        }
        socket.is_active = true;
        let _lock = self.lock.lock();
        self.active_socket.insert_tail(&mut socket.list);
        drop(_lock);
        Ok(())
    }

    pub fn activate_waiting_socket(
        &'static mut self,
        socket: &mut Socket,
        allow_sleep: bool,
    ) -> Result<&'static mut Socket, NetworkError> {
        let _socket_lock = socket.lock.lock();

        if let Some(waiting_socket) = unsafe {
            socket
                .waiting_socket
                .take_first_entry(offset_of!(Socket, list))
        } {
            drop(_socket_lock);
            waiting_socket.list = PtrLinkedListNode::new();
            let _lock = self.lock.lock();

            self.active_socket.insert_tail(&mut waiting_socket.list);
            drop(_lock);
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
        return Ok(read_size);
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
                InternetType::None => {
                    return Err(NetworkError::InvalidSocket);
                }
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
                InternetType::None => {
                    return Err(NetworkError::InvalidSocket);
                }
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
                    return result.and_then(|_| Ok(buffer_size));
                }
                InternetType::Ipv6(_) => {
                    unimplemented!()
                }
            },
        }
    }

    pub fn close_socket(&mut self, socket: &'static mut Socket) -> Result<(), NetworkError> {
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
        } {
            let _child_socket_lock = child_socket.lock.lock();
            if child_socket.is_active {
                drop(_child_socket_lock);
                /* child_socket will be closed by the owner */
            } else {
                self._close_socket(child_socket)?;
                drop(_child_socket_lock);
                let _ = kfree!(child_socket);
            }
        }

        let _ = socket.wait_queue.wakeup_all();
        drop(_socket_lock);
        return Ok(());
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
        let _lock = self.lock.lock();
        for e in unsafe { self.active_socket.iter_mut(offset_of!(Socket, list)) } {
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
                            let InternetType::Ipv4(arrive_ipv4_info) = &transport_info else {break;};

                            if ipv4_info.get_their_address() != ipv4::IPV4_ADDRESS_ANY
                                && ipv4_info.get_their_address() != ipv4::IPV4_BROAD_CAST
                            {
                                if ipv4_info.get_their_address()
                                    != arrive_ipv4_info.get_sender_address()
                                {
                                    break;
                                }
                            }
                            if ipv4_info.get_our_address() != ipv4::IPV4_ADDRESS_ANY
                                && arrive_ipv4_info.get_destination_address()
                                    != ipv4::IPV4_BROAD_CAST
                            {
                                if ipv4_info.get_our_address()
                                    != arrive_ipv4_info.get_destination_address()
                                {
                                    break;
                                }
                            }
                        }
                        InternetType::Ipv6(_) => {
                            unimplemented!()
                        }
                    }
                    drop(_lock);
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
                                let _ = kfree!(data_buffer, data_length);
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
                    if let Err(err) = e.wait_queue.wakeup_all() {
                        pr_err!("Failed to wake up threads: {:?}", err);
                    }
                    drop(_socket_lock);
                    let _ = kfree!(data_buffer, data_length);
                    return;
                }
            }
        }
        drop(_lock);
        pr_debug!("UDP segment will be deleted...");
        let _ = kfree!(data_buffer, data_length);
    }

    /* TCP Port Open Handler */
    pub(super) fn tcp_port_open_handler(
        &mut self,
        link_info: LinkType,
        internet_info: InternetType,
        tcp_segment_info: &tcp::TcpSegmentInfo,
        new_session_info: tcp::TcpSessionInfo,
    ) -> Result<(), NetworkError> {
        let _lock = self.lock.lock();
        for e in unsafe { self.active_socket.iter_mut(offset_of!(Socket, list)) } {
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
                            let InternetType::Ipv4(arrive_ipv4_info) = &internet_info else {break;};

                            if ipv4_info.get_their_address() != ipv4::IPV4_ADDRESS_ANY
                                && ipv4_info.get_their_address() != ipv4::IPV4_BROAD_CAST
                            {
                                if ipv4_info.get_their_address()
                                    != arrive_ipv4_info.get_sender_address()
                                {
                                    break;
                                }
                            }
                            if ipv4_info.get_our_address() != ipv4::IPV4_ADDRESS_ANY
                                && arrive_ipv4_info.get_destination_address()
                                    != ipv4::IPV4_BROAD_CAST
                            {
                                if ipv4_info.get_our_address()
                                    != arrive_ipv4_info.get_destination_address()
                                {
                                    break;
                                }
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
                    e.waiting_socket.insert_tail(&mut child_socket.list);
                    if let Err(err) = e.wait_queue.wakeup_all() {
                        pr_err!("Failed to wake up threads: {:?}", err);
                    }
                    drop(_socket_lock);
                    return Ok(());
                }
            }
        }
        drop(_lock);
        return Err(NetworkError::InvalidAddress);
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
        let _lock = self.lock.lock();
        for e in unsafe { self.active_socket.iter_mut(offset_of!(Socket, list)) } {
            if let TransportType::Tcp(tcp_info) = &mut e.layer_info.transport {
                if e.is_active
                    && tcp_segment_info.get_destination_port() == tcp_info.get_our_port()
                    && tcp_info.get_their_port() == tcp_segment_info.get_sender_port()
                {
                    match &e.layer_info.internet {
                        InternetType::None => { /* Check: OK */ }
                        InternetType::Ipv4(ipv4_info) => {
                            let InternetType::Ipv4(arrive_ipv4_info) = &internet_info else {break;};
                            if ipv4_info.get_their_address() != ipv4::IPV4_ADDRESS_ANY
                                && ipv4_info.get_their_address() != ipv4::IPV4_BROAD_CAST
                            {
                                if ipv4_info.get_their_address()
                                    != arrive_ipv4_info.get_sender_address()
                                {
                                    break;
                                }
                            }
                            if ipv4_info.get_our_address() != ipv4::IPV4_ADDRESS_ANY
                                && arrive_ipv4_info.get_destination_address()
                                    != ipv4::IPV4_BROAD_CAST
                            {
                                if ipv4_info.get_our_address()
                                    != arrive_ipv4_info.get_destination_address()
                                {
                                    break;
                                }
                            }
                        }
                        InternetType::Ipv6(_) => {
                            unimplemented!()
                        }
                    }
                    drop(_lock);
                    let _socket_lock = e.lock.lock();
                    let result =
                        update_function(tcp_info).and_then(|active| Ok(e.is_active = active));
                    let _ = e.wait_queue.wakeup_all();
                    drop(_socket_lock);
                    return result;
                }
            }
        }
        drop(_lock);
        return Err(NetworkError::InvalidAddress);
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
        let _lock = self.lock.lock();
        for e in unsafe { self.active_socket.iter_mut(offset_of!(Socket, list)) } {
            if let TransportType::Tcp(tcp_info) = &mut e.layer_info.transport {
                if e.is_active
                    && tcp_info.get_status() == tcp::TcpSessionStatus::Opened
                    && tcp_segment_info.get_destination_port() == tcp_info.get_our_port()
                    && tcp_info.get_their_port() == tcp_segment_info.get_sender_port()
                {
                    match &e.layer_info.internet {
                        InternetType::None => { /* Check: OK */ }
                        InternetType::Ipv4(ipv4_info) => {
                            let InternetType::Ipv4(arrive_ipv4_info) = &internet_info else {break;};
                            if ipv4_info.get_their_address() != ipv4::IPV4_ADDRESS_ANY
                                && ipv4_info.get_their_address() != ipv4::IPV4_BROAD_CAST
                            {
                                if ipv4_info.get_their_address()
                                    != arrive_ipv4_info.get_sender_address()
                                {
                                    break;
                                }
                            }
                            if ipv4_info.get_our_address() != ipv4::IPV4_ADDRESS_ANY
                                && arrive_ipv4_info.get_destination_address()
                                    != ipv4::IPV4_BROAD_CAST
                            {
                                if ipv4_info.get_our_address()
                                    != arrive_ipv4_info.get_destination_address()
                                {
                                    break;
                                }
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
                    if let Err(err) = e.wait_queue.wakeup_all() {
                        pr_err!("Failed to wake up threads: {:?}", err);
                    }
                    drop(_socket_lock);

                    return result;
                }
            }
        }
        drop(_lock);
        return Err(NetworkError::InvalidAddress);
    }
}
