//!
//! Socket Manager
//!

use super::{tcp, AddressInfo};

use crate::kernel::collections::ptr_linked_list::{PtrLinkedList, PtrLinkedListNode};
use crate::kernel::collections::ring_buffer::Ringbuffer;
use crate::kernel::file_manager::{
    File, FileDescriptor, FileOperationDriver, FileSeekOrigin, FILE_PERMISSION_READ,
    FILE_PERMISSION_WRITE,
};
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{Address, MSize, VAddress};
use crate::kernel::sync::spin_lock::SpinLockFlag;
use crate::kernel::task_manager::wait_queue::WaitQueue;

use crate::{kfree, kmalloc};

const DEFAULT_BUFFER_SIZE: usize = 4096;

pub struct SocketManager {
    lock: SpinLockFlag,
    active_socket: PtrLinkedList<SocketInfo>,
    listening_socket: PtrLinkedList<SocketInfo>,
}

struct ListeningInfo {
    address: AddressInfo,
    port: u16,
    waiting_socket: PtrLinkedList<SocketInfo>,
}

enum SegmentInfo {
    Invalid,
    Listening(ListeningInfo),
    Tcp(tcp::TcpSegmentInfo),
}

pub struct SocketInfo {
    list: PtrLinkedListNode<Self>,
    lock: SpinLockFlag,
    segment_info: SegmentInfo,
    domain: Domain,
    protocol: Protocol,
    wait_queue: WaitQueue,
    send_ring_buffer: Ringbuffer,
    receive_ring_buffer: Ringbuffer,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Domain {
    Unix,
    Ipv4,
    Ipv6,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Protocol {
    Raw,
    Tcp,
    Udp,
}

impl SocketManager {
    pub fn new() -> Self {
        Self {
            lock: SpinLockFlag::new(),
            active_socket: PtrLinkedList::new(),
            listening_socket: PtrLinkedList::new(),
        }
    }

    pub fn create_socket_as_file(
        &'static mut self,
        domain: Domain,
        protocol: Protocol,
    ) -> Result<File, ()> {
        match kmalloc!(
            SocketInfo,
            SocketInfo {
                list: PtrLinkedListNode::new(),
                lock: SpinLockFlag::new(),
                domain,
                protocol,
                segment_info: SegmentInfo::Invalid,
                wait_queue: WaitQueue::new(),
                send_ring_buffer: Ringbuffer::new(),
                receive_ring_buffer: Ringbuffer::new()
            }
        ) {
            Ok(d) => Ok(File::new(
                FileDescriptor::new(d as *mut _ as usize, 0, 0),
                self,
            )),
            Err(e) => {
                pr_err!("Failed to allocate memory: {:?}", e);
                return Err(());
            }
        }
    }

    pub fn bind_socket(
        &mut self,
        file: &mut File,
        address: AddressInfo,
        port: u16,
    ) -> Result<(), ()> {
        if file.get_driver_address() != self as *mut _ as usize {
            pr_err!("Invalid file descriptor");
            return Err(());
        }
        let file_descriptor = file.get_descriptor();
        let socket_info = unsafe { &mut *(file_descriptor.get_data() as *mut SocketInfo) };
        if !matches!(socket_info.segment_info, SegmentInfo::Invalid) {
            pr_err!("Socket is already used");
            return Err(());
        }
        if address != AddressInfo::Any
            && ((socket_info.domain == Domain::Ipv4 && !matches!(AddressInfo::Ipv4, address))
                || (socket_info.domain == Domain::Ipv6 && !matches!(AddressInfo::Ipv6, address)))
        {
            pr_err!("Invalid address");
            return Err(());
        }

        socket_info.segment_info = SegmentInfo::Listening(ListeningInfo {
            address,
            port,
            waiting_socket: PtrLinkedList::new(),
        });
        return Ok(());
    }

    pub fn listen_socket(&mut self, file: &mut File, max_connection: usize) -> Result<(), ()> {
        if file.get_driver_address() != self as *mut _ as usize {
            pr_err!("Invalid file descriptor");
            return Err(());
        }
        let file_descriptor = file.get_descriptor();
        let socket_info = unsafe { &mut *(file_descriptor.get_data() as *mut SocketInfo) };

        let _socket_lock = socket_info.lock.lock();
        if let SegmentInfo::Listening(listen_info) = &socket_info.segment_info {
            match socket_info.protocol {
                Protocol::Raw => Err(()),
                Protocol::Tcp => {
                    let result = tcp::bind_port(
                        listen_info.port,
                        listen_info.address.clone(),
                        max_connection,
                        Self::tcp_open_handler,
                    );
                    if let Err(err) = result {
                        pr_err!("Failed to listen TCP socket: {:?}", err);
                        return Err(());
                    }
                    socket_info.list = PtrLinkedListNode::new();
                    // Socket info is not inserted yet, so we can lock self after socket.
                    let _self_lock = self.lock.lock();
                    self.listening_socket.insert_tail(&mut socket_info.list);
                    drop(_socket_lock);
                    drop(_self_lock);
                    result
                }
                Protocol::Udp => Err(()),
            }
        } else {
            drop(_socket_lock);
            pr_err!("Socket is not listenable");
            return Err(());
        }
    }

    pub fn accept_client(&'static mut self, listening_socket_file: &mut File) -> Result<File, ()> {
        if listening_socket_file.get_driver_address() != self as *mut _ as usize {
            pr_err!("Invalid file descriptor");
            return Err(());
        }
        let socket_info =
            unsafe { &mut *(listening_socket_file.get_descriptor().get_data() as *mut SocketInfo) };

        let mut _socket_lock = socket_info.lock.lock();
        if let SegmentInfo::Listening(listen_info) = &mut socket_info.segment_info {
            match socket_info.protocol {
                Protocol::Raw => Err(()),
                Protocol::Tcp => {
                    while listen_info.waiting_socket.is_empty() {
                        /* Is the timing of unlock OK? */
                        drop(_socket_lock);
                        let _ = socket_info.wait_queue.add_current_thread();
                        _socket_lock = socket_info.lock.lock();
                    }
                    let child_socket = unsafe {
                        listen_info
                            .waiting_socket
                            .take_first_entry(offset_of!(SocketInfo, list))
                    }
                    .unwrap();
                    drop(_socket_lock);
                    // child_socket is not inserted yet, so we can lock self after socket.
                    child_socket.list = PtrLinkedListNode::new();
                    let _self_lock = self.lock.lock();
                    self.active_socket.insert_tail(&mut child_socket.list);
                    drop(_self_lock);
                    Ok(File::new(
                        FileDescriptor::new(
                            child_socket as *mut _ as usize,
                            0,
                            FILE_PERMISSION_READ | FILE_PERMISSION_WRITE,
                        ),
                        self,
                    ))
                }
                Protocol::Udp => Err(()),
            }
        } else {
            drop(_socket_lock);
            pr_err!("Socket is not listening socket");
            Err(())
        }
    }

    pub fn read_data(
        &mut self,
        socket_file: &mut File,
        buffer_address: VAddress,
        buffer_size: MSize,
    ) -> Result<MSize, ()> {
        if !socket_file.is_readable() {
            pr_debug!("Socket is not readable",);
            return Err(());
        }
        if socket_file.get_driver_address() != self as *mut _ as usize {
            pr_err!("Invalid file descriptor");
            return Err(());
        }
        let file_descriptor = socket_file.get_descriptor();
        let socket_info = unsafe { &mut *(file_descriptor.get_data() as *mut SocketInfo) };
        let mut _lock = socket_info.lock.lock();
        let mut read_size: MSize;
        loop {
            read_size = socket_info
                .receive_ring_buffer
                .read(buffer_address, buffer_size);
            if !read_size.is_zero() {
                break;
            }
            drop(_lock);
            if let Err(err) = socket_info.wait_queue.add_current_thread() {
                pr_err!("Failed to sleep: {:?}", err);
                return Err(());
            }
            _lock = socket_info.lock.lock();
        }
        return Ok(read_size);
    }

    pub fn write_data(
        &mut self,
        socket_file: &mut File,
        buffer_address: VAddress,
        buffer_size: MSize,
    ) -> Result<MSize, ()> {
        if !socket_file.is_writable() {
            pr_debug!("Socket is not writable",);
            return Err(());
        }
        if socket_file.get_driver_address() != self as *mut _ as usize {
            pr_err!("Invalid file descriptor");
            return Err(());
        }
        let file_descriptor = socket_file.get_descriptor();
        let socket_info = unsafe { &mut *(file_descriptor.get_data() as *mut SocketInfo) };
        let _lock = socket_info.lock.lock();
        match &mut socket_info.segment_info {
            SegmentInfo::Invalid => Err(()),
            SegmentInfo::Listening(_) => Err(()),
            SegmentInfo::Tcp(segment) => {
                tcp::send_data(segment, buffer_address, buffer_size)?;
                Ok(buffer_size)
            }
        }
    }

    pub fn tcp_data_handler(
        data_base_address: VAddress,
        data_length: MSize,
        segment_info: tcp::TcpSegmentInfo,
    ) -> Result<(), ()> {
        let s = get_kernel_manager_cluster()
            .network_manager
            .get_socket_manager();
        let _self_lock = s.lock.lock();
        for e in unsafe { s.active_socket.iter_mut(offset_of!(SocketInfo, list)) } {
            if e.protocol == Protocol::Tcp {
                if let SegmentInfo::Tcp(info) = &mut e.segment_info {
                    if segment_info.is_equal(info) {
                        let _sock_lock = e.lock.lock();
                        if e.receive_ring_buffer.get_buffer_size().is_zero() {
                            let new_buffer_size = MSize::new(DEFAULT_BUFFER_SIZE);
                            match kmalloc!(new_buffer_size) {
                                Ok(a) => {
                                    e.receive_ring_buffer.set_new_buffer(a, new_buffer_size);
                                }
                                Err(err) => {
                                    pr_err!("Failed to allocate memory: {:?}", err);
                                    return Err(());
                                }
                            }
                        }
                        let written_size =
                            e.receive_ring_buffer.write(data_base_address, data_length);
                        let _ = e.wait_queue.wakeup_all();
                        drop(_sock_lock);
                        drop(_self_lock);
                        if written_size < data_length {
                            pr_err!("{} bytes are overflowed.", (data_length - written_size));
                        }
                        return Ok(());
                    }
                }
            }
        }
        drop(_self_lock);
        return Err(());
    }

    pub fn tcp_open_handler(segment_info: tcp::TcpSegmentInfo) -> Result<tcp::TcpDataHandler, ()> {
        let s = get_kernel_manager_cluster()
            .network_manager
            .get_socket_manager();
        let _self_lock = s.lock.lock();
        for e in unsafe { s.listening_socket.iter_mut(offset_of!(SocketInfo, list)) } {
            if e.protocol == Protocol::Tcp {
                if let SegmentInfo::Listening(info) = &mut e.segment_info {
                    if info.port == segment_info.get_destination_port()
                        && (info.address == AddressInfo::Any
                            || info.address
                                == AddressInfo::Ipv4(
                                    segment_info.get_packet_info().get_destination_address(),
                                ))
                    {
                        let _socket_lock = e.lock.lock();
                        /* Create Socket */
                        let child_socket = kmalloc!(
                            SocketInfo,
                            SocketInfo {
                                list: PtrLinkedListNode::new(),
                                lock: SpinLockFlag::new(),
                                segment_info: SegmentInfo::Tcp(segment_info),
                                domain: e.domain,
                                protocol: e.protocol,
                                wait_queue: WaitQueue::new(),
                                send_ring_buffer: Ringbuffer::new(),
                                receive_ring_buffer: Ringbuffer::new(),
                            }
                        );
                        if let Err(err) = child_socket {
                            pr_err!("Failed to allocate memory: {:?}", err);
                            return Err(());
                        }
                        let child_socket = child_socket.unwrap();
                        info.waiting_socket.insert_tail(&mut child_socket.list);
                        let _ = e.wait_queue.wakeup_one();
                        drop(_socket_lock);
                        drop(_self_lock);
                        return Ok(Self::tcp_data_handler);
                    }
                }
            }
        }
        return Err(());
    }

    fn _close_socket(&mut self, socket_info: &mut SocketInfo, is_active: bool) -> Result<(), ()> {
        assert!(self.lock.is_locked());
        let _socket_lock = socket_info.lock.lock();
        if !socket_info.send_ring_buffer.get_buffer_size().is_zero() {
            let _ = kfree!(
                socket_info.send_ring_buffer.get_buffer_address(),
                socket_info.send_ring_buffer.get_buffer_size()
            );
            socket_info.send_ring_buffer.unset_buffer();
        }
        if !socket_info.receive_ring_buffer.get_buffer_size().is_zero() {
            let _ = kfree!(
                socket_info.receive_ring_buffer.get_buffer_address(),
                socket_info.receive_ring_buffer.get_buffer_size()
            );
            socket_info.receive_ring_buffer.unset_buffer();
        }
        match &mut socket_info.segment_info {
            SegmentInfo::Invalid => { /* Do nothing */ }
            SegmentInfo::Listening(l) => {
                match socket_info.protocol {
                    Protocol::Raw => { /*TODO: ... */ }
                    Protocol::Tcp => {
                        tcp::unbind_port(l.port, l.address.clone())?;
                    }
                    Protocol::Udp => { /*TODO: ... */ }
                }
                while let Some(e) = unsafe {
                    l.waiting_socket
                        .take_first_entry(offset_of!(SocketInfo, list))
                } {
                    self._close_socket(e, false)?;
                    let _ = kfree!(e);
                }
            }
            SegmentInfo::Tcp(segment_info) => {
                tcp::close_session(segment_info);
                if is_active {
                    self.active_socket.remove(&mut socket_info.list);
                }
            }
        }
        socket_info.segment_info = SegmentInfo::Invalid;
        let _ = socket_info.wait_queue.wakeup_all();
        drop(_socket_lock);
        return Ok(());
    }
}

impl FileOperationDriver for SocketManager {
    fn read(
        &mut self,
        _descriptor: &mut FileDescriptor,
        _buffer: VAddress,
        _length: usize,
    ) -> Result<usize, ()> {
        return Err(());
    }

    fn write(
        &mut self,
        _descriptor: &mut FileDescriptor,
        _buffer: VAddress,
        _length: usize,
    ) -> Result<usize, ()> {
        return Err(());
    }

    fn seek(&mut self, _: &mut FileDescriptor, _: usize, _: FileSeekOrigin) -> Result<usize, ()> {
        return Err(());
    }

    fn close(&mut self, descriptor: FileDescriptor) {
        let _self_lock = self.lock.lock();
        let socket_info = unsafe { &mut *(descriptor.get_data() as *mut SocketInfo) };
        if let Err(err) = self._close_socket(socket_info, true) {
            pr_err!("Failed to close socket: {:?}", err);
        } else {
            let _ = kfree!(socket_info);
        }
    }
}
