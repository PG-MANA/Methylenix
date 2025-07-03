//!
//! Functions to control socket from system call
//!

use super::{
    super::{
        InternetType, LinkType, TransportType,
        ipv4::{IPV4_ADDRESS_ANY, Ipv4ConnectionInfo},
        tcp::{TCP_PORT_ANY, TcpSessionInfo},
        udp::{UDP_PORT_ANY, UdpConnectionInfo},
    },
    Socket,
};
use crate::kernel::{
    file_manager::{
        FILE_PERMISSION_READ, FILE_PERMISSION_WRITE, File, FileDescriptor, FileDescriptorData,
        FileError, FileOperationDriver, FileSeekOrigin,
    },
    manager_cluster::get_kernel_manager_cluster,
    memory_manager::{
        data_type::{MOffset, MSize, VAddress},
        kmalloc,
    },
};

const AF_UNIX: u64 = 0x01;
const AF_INET: u64 = 0x02;
const AF_INET6: u64 = 0x0A;
const SOCK_STREAM: u64 = 0x01;
const SOCK_DGRAM: u64 = 0x02;

const INADDR_ANY: [u8; 4] = 0u32.to_be_bytes();

const DEVICE_ID_VALID: usize = 0;
const DEVICE_ID_INVALID: usize = usize::MAX;

#[repr(C)]
pub struct SockAddr {
    sa_family: u16,
    sa_data: [u8; 14],
}

#[repr(C)]
struct SockAddrIn {
    sin_family: u16,
    sin_port: u16,
    sin_addr: [u8; 4],
    sin_zero: [u8; 8],
}

#[repr(transparent)]
struct NetworkSocketDriver {}

static mut NETWORK_SOCKET_DRIVER: NetworkSocketDriver = NetworkSocketDriver {};

fn get_socket_driver_mut() -> &'static mut NetworkSocketDriver {
    unsafe { &mut *core::ptr::addr_of_mut!(NETWORK_SOCKET_DRIVER) }
}

pub fn create_socket(
    domain_number: u64,
    socket_type_number: u64,
    protocol_number: u64,
) -> Result<File<'static>, ()> {
    let link_type = LinkType::None;
    let internet_type = match domain_number {
        AF_INET => InternetType::Ipv4(Ipv4ConnectionInfo::new(IPV4_ADDRESS_ANY, IPV4_ADDRESS_ANY)),
        AF_INET6 => InternetType::Ipv6(()),
        AF_UNIX => InternetType::None,
        _ => {
            pr_err!("Unknown domain: {:#X}", domain_number);
            return Err(());
        }
    };
    let transport_type = match protocol_number {
        0 => match socket_type_number {
            SOCK_STREAM => TransportType::Tcp(TcpSessionInfo::new(TCP_PORT_ANY, TCP_PORT_ANY)),
            SOCK_DGRAM => TransportType::Udp(UdpConnectionInfo::new(UDP_PORT_ANY, UDP_PORT_ANY)),
            _ => {
                pr_err!("Unknown socket_type: {:#X}", socket_type_number);
                return Err(());
            }
        },
        _ => {
            pr_err!("Unknown protocol: {:#X}", protocol_number);
            return Err(());
        }
    };

    match get_kernel_manager_cluster()
        .network_manager
        .get_socket_manager()
        .create_socket(link_type, internet_type, transport_type)
    {
        Ok(socket) => match kmalloc!(Socket, socket) {
            Ok(d) => Ok(File::new(
                FileDescriptor::new(d as *mut _ as usize, DEVICE_ID_INVALID, 0),
                get_socket_driver_mut(),
            )),
            Err(err) => {
                pr_err!("Failed to allocate memory: {:?}", err);
                Err(())
            }
        },
        Err(err) => {
            pr_err!("Failed to create socket: {:?}", err);
            Err(())
        }
    }
}

pub fn bind_socket(file: &mut File, sock_addr: &SockAddr) -> Result<(), ()> {
    if file.get_driver_address() != get_socket_driver_mut() as *mut _ as usize {
        pr_err!("Invalid file descriptor");
        return Err(());
    }
    let file_descriptor = file.get_descriptor();
    if file_descriptor.get_device_index() != DEVICE_ID_INVALID {
        pr_err!("Socket is in use");
        return Err(());
    }
    let socket = unsafe { &mut *(file_descriptor.get_data() as *mut Socket) };

    match &mut socket.layer_info.internet {
        InternetType::None => {
            unimplemented!()
        }
        InternetType::Ipv4(ipv4_info) => {
            if sock_addr.sa_family as u64 != AF_INET {
                pr_err!("Invalid Family");
                return Err(());
            }
            let sock_addr_in = unsafe { core::mem::transmute::<&SockAddr, &SockAddrIn>(sock_addr) };
            let our_address = if sock_addr_in.sin_addr == INADDR_ANY {
                IPV4_ADDRESS_ANY
            } else {
                u32::from_be_bytes(sock_addr_in.sin_addr)
            };
            let port = u16::from_be(sock_addr_in.sin_port);
            *ipv4_info = Ipv4ConnectionInfo::new(our_address, IPV4_ADDRESS_ANY);
            match &mut socket.layer_info.transport {
                TransportType::Tcp(tcp_session) => {
                    *tcp_session = TcpSessionInfo::new(port, TCP_PORT_ANY)
                }
                TransportType::Udp(udp_connection) => {
                    *udp_connection = UdpConnectionInfo::new(port, UDP_PORT_ANY)
                }
            };
        }
        InternetType::Ipv6(_) => {
            unimplemented!()
        }
    }
    Ok(())
}

pub fn listen_socket(file: &mut File, _max_connection: usize) -> Result<(), ()> {
    if file.get_driver_address() != get_socket_driver_mut() as *mut _ as usize {
        pr_err!("Invalid file descriptor");
        return Err(());
    }
    let file_descriptor = file.get_descriptor();
    if file_descriptor.get_device_index() != DEVICE_ID_INVALID {
        pr_err!("Socket is in use");
        return Err(());
    }
    let socket = unsafe { &mut *(file_descriptor.get_data() as *mut Socket) };
    match get_kernel_manager_cluster()
        .network_manager
        .get_socket_manager()
        .add_listening_socket(socket)
    {
        Ok(_) => {
            *file = File::new(
                FileDescriptor::new(file_descriptor.get_data(), DEVICE_ID_VALID, 0),
                get_socket_driver_mut(),
            );
            Ok(())
        }
        Err(err) => {
            pr_err!("Failed to add socket to listen: {:?}", err);
            Err(())
        }
    }
}

pub fn accept(file: &mut File) -> Result<(File<'static>, SockAddr), ()> {
    if file.get_driver_address() != get_socket_driver_mut() as *mut _ as usize {
        pr_err!("Invalid file descriptor");
        return Err(());
    }
    let file_descriptor = file.get_descriptor();
    if file_descriptor.get_device_index() != DEVICE_ID_VALID {
        pr_err!("Socket is invalid");
        return Err(());
    }
    let socket = unsafe { &mut *(file_descriptor.get_data() as *mut Socket) };
    match get_kernel_manager_cluster()
        .network_manager
        .get_socket_manager()
        .activate_waiting_socket(socket, true)
    {
        Ok(s) => {
            let sock_addr = match &s.layer_info.internet {
                InternetType::None => SockAddr {
                    sa_family: 0,
                    sa_data: [0; 14],
                },
                InternetType::Ipv4(ipv_info) => {
                    let sock_addr_in = SockAddrIn {
                        sin_family: AF_INET as u16,
                        sin_port: match &s.layer_info.transport {
                            TransportType::Tcp(tcp_session_info) => {
                                tcp_session_info.get_their_port()
                            }
                            TransportType::Udp(udp_connection) => udp_connection.get_their_port(),
                        },
                        sin_addr: ipv_info.get_their_address().to_be_bytes(),
                        sin_zero: [0u8; 8],
                    };
                    unsafe { core::mem::transmute::<SockAddrIn, SockAddr>(sock_addr_in) }
                }
                InternetType::Ipv6(_) => {
                    unimplemented!()
                }
            };
            let accepted_file = File::new(
                FileDescriptor::new(
                    s as *mut _ as usize,
                    DEVICE_ID_VALID,
                    FILE_PERMISSION_READ | FILE_PERMISSION_WRITE,
                ),
                get_socket_driver_mut(),
            );
            Ok((accepted_file, sock_addr))
        }
        Err(err) => {
            pr_err!("Failed to accept socket: {:?}", err);
            Err(())
        }
    }
}

pub fn recv_from(
    socket_file: &mut File,
    buffer_address: VAddress,
    buffer_size: MSize,
    flags: usize,
    sock_addr: Option<&SockAddr>,
) -> Result<MSize, ()> {
    if socket_file.get_driver_address() != get_socket_driver_mut() as *mut _ as usize {
        pr_err!("Invalid file descriptor");
        return Err(());
    } else if !socket_file.is_readable() {
        pr_err!("Socket is not readable");
        return Err(());
    }
    _recv_from(
        socket_file.get_descriptor(),
        buffer_address,
        buffer_size,
        flags,
        sock_addr,
    )
}

fn _recv_from(
    file_descriptor: &FileDescriptor,
    buffer_address: VAddress,
    buffer_size: MSize,
    _flags: usize,
    _sock_addr: Option<&SockAddr>,
) -> Result<MSize, ()> {
    if file_descriptor.get_device_index() != DEVICE_ID_VALID {
        pr_err!("Socket is invalid");
        return Err(());
    }
    let socket = unsafe { &mut *(file_descriptor.get_data() as *mut Socket) };

    match get_kernel_manager_cluster()
        .network_manager
        .get_socket_manager()
        .read_socket(socket, buffer_address, buffer_size, true)
    {
        Ok(size) => Ok(size),
        Err(err) => {
            pr_err!("Failed to read: {:?}", err);
            Err(())
        }
    }
}

pub fn send_to(
    socket_file: &mut File,
    buffer_address: VAddress,
    buffer_size: MSize,
    flags: usize,
    sock_addr: Option<&SockAddr>,
) -> Result<MSize, ()> {
    if socket_file.get_driver_address() != get_socket_driver_mut() as *mut _ as usize {
        pr_err!("Invalid file descriptor");
        return Err(());
    } else if !socket_file.is_writable() {
        pr_err!("Socket is not writable");
        return Err(());
    }
    _send_to(
        socket_file.get_descriptor(),
        buffer_address,
        buffer_size,
        flags,
        sock_addr,
    )
}

fn _send_to(
    file_descriptor: &FileDescriptor,
    buffer_address: VAddress,
    buffer_size: MSize,
    _flags: usize,
    _sock_addr: Option<&SockAddr>,
) -> Result<MSize, ()> {
    if file_descriptor.get_device_index() != DEVICE_ID_VALID {
        pr_err!("Socket is invalid");
        return Err(());
    }
    let socket = unsafe { &mut *(file_descriptor.get_data() as *mut Socket) };

    match get_kernel_manager_cluster()
        .network_manager
        .get_socket_manager()
        .send_socket(socket, buffer_address, buffer_size)
    {
        Ok(size) => Ok(size),
        Err(err) => {
            pr_err!("Failed to send: {:?}", err);
            Err(())
        }
    }
}

impl FileOperationDriver for NetworkSocketDriver {
    fn read(
        &mut self,
        descriptor: &mut FileDescriptor,
        buffer: VAddress,
        length: MSize,
    ) -> Result<MSize, FileError> {
        _recv_from(descriptor, buffer, length, 0, None).or(Err(FileError::DeviceError))
    }

    fn write(
        &mut self,
        descriptor: &mut FileDescriptor,
        buffer: VAddress,
        length: MSize,
    ) -> Result<MSize, FileError> {
        _send_to(descriptor, buffer, length, 0, None).or(Err(FileError::DeviceError))
    }

    fn seek(
        &mut self,
        _descriptor: &mut FileDescriptor,
        _offset: MOffset,
        _origin: FileSeekOrigin,
    ) -> Result<MOffset, FileError> {
        Err(FileError::OperationNotSupported)
    }

    fn close(&mut self, descriptor: FileDescriptor) {
        let socket = unsafe { &mut *(descriptor.get_data() as *mut Socket) };
        if descriptor.get_device_index() == DEVICE_ID_INVALID {
            let _ = kfree!(socket);
        } else if let Err(err) = get_kernel_manager_cluster()
            .network_manager
            .get_socket_manager()
            .close_socket(socket)
        {
            pr_err!("Failed to close socket: {:?}", err);
        }
    }
}
