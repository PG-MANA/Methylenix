//!
//! SystemCall Network
//!

use crate::kernel::file_manager::File;
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{MSize, VAddress};
use crate::kernel::network_manager::socket_manager::{Domain, Protocol};
use crate::kernel::network_manager::AddressInfo;

const AF_UNIX: u64 = 0x01;
const AF_INET: u64 = 0x02;
const AF_INET6: u64 = 0x0A;
const SOCK_STREAM: u64 = 0x01;
const SOCK_DGRAM: u64 = 0x02;

const INADDR_ANY: [u8; 4] = 0u32.to_be_bytes();

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

pub fn create_socket(
    domain_number: u64,
    socket_type_number: u64,
    protocol_number: u64,
) -> Result<File<'static>, ()> {
    let domain = match domain_number {
        AF_INET => Domain::Ipv4,
        AF_INET6 => Domain::Ipv6,
        AF_UNIX => Domain::Unix,
        _ => {
            pr_err!("Unknown domain: {:#X}", domain_number);
            return Err(());
        }
    };
    let protocol = match protocol_number {
        0 => match socket_type_number {
            SOCK_STREAM => Protocol::Tcp,
            SOCK_DGRAM => Protocol::Udp,
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
    get_kernel_manager_cluster()
        .network_manager
        .get_socket_manager()
        .create_socket_as_file(domain, protocol)
}

pub fn bind_socket(socket_file: &mut File, sock_addr: &SockAddr) -> Result<(), ()> {
    let address_info: AddressInfo;
    let port: u16;

    match sock_addr.sa_family as u64 {
        AF_INET => {
            let sock_addr_in = unsafe { core::mem::transmute::<&SockAddr, &SockAddrIn>(sock_addr) };
            address_info = if sock_addr_in.sin_addr == INADDR_ANY {
                AddressInfo::Any
            } else {
                AddressInfo::Ipv4(u32::from_be_bytes(sock_addr_in.sin_addr.clone()))
            };
            port = u16::from_be(sock_addr_in.sin_port);
        }
        _ => {
            pr_err!("Unsupported socket family: {:#X}", sock_addr.sa_family);
            return Err(());
        }
    }
    get_kernel_manager_cluster()
        .network_manager
        .get_socket_manager()
        .bind_socket(socket_file, address_info, port)
}

pub fn listen_socket(socket_file: &mut File, max_connection: usize) -> Result<(), ()> {
    get_kernel_manager_cluster()
        .network_manager
        .get_socket_manager()
        .listen_socket(socket_file, max_connection)
}

pub fn accept<'a>(socket_file: &'a mut File) -> Result<(File<'static>, SockAddr), ()> {
    let file = get_kernel_manager_cluster()
        .network_manager
        .get_socket_manager()
        .accept_client(socket_file)?;
    let sock_addr = SockAddr {
        sa_family: 0,
        sa_data: [0; 14],
    };
    Ok((file, sock_addr))
}

pub fn recv_from(
    socket_file: &mut File,
    buffer_address: VAddress,
    buffer_size: MSize,
    _flags: usize,
    _sock_addr: Option<&SockAddr>,
) -> Result<MSize, ()> {
    get_kernel_manager_cluster()
        .network_manager
        .get_socket_manager()
        .read_data(socket_file, buffer_address, buffer_size)
}

pub fn send_to(
    socket_file: &mut File,
    buffer_address: VAddress,
    buffer_size: MSize,
    _flags: usize,
    _sock_addr: Option<&SockAddr>,
) -> Result<MSize, ()> {
    get_kernel_manager_cluster()
        .network_manager
        .get_socket_manager()
        .write_data(socket_file, buffer_address, buffer_size)
}
