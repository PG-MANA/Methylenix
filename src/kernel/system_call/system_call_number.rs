//!
//! System Call Number List
//!

pub type SysCallNumber = u64;

pub const SYSCALL_EXIT: SysCallNumber = 0x3C;
pub const SYSCALL_EXIT_GROUP: SysCallNumber = 0xE7;
pub const SYSCALL_READ: SysCallNumber = 0x00;
pub const SYSCALL_WRITE: SysCallNumber = 0x01;
pub const SYSCALL_OPEN: SysCallNumber = 0x02;
pub const SYSCALL_CLOSE: SysCallNumber = 0x03;
pub const SYSCALL_WRITEV: SysCallNumber = 0x14;
pub const SYSCALL_ARCH_PRCTL: SysCallNumber = 0x9E;
pub const SYSCALL_SET_TID_ADDRESS: SysCallNumber = 0xDA;
pub const SYSCALL_BRK: SysCallNumber = 0x0C;
pub const SYSCALL_MMAP: SysCallNumber = 0x09;
pub const SYSCALL_MUNMAP: SysCallNumber = 0x0B;

pub const SYSCALL_SOCKET: SysCallNumber = 0x29;
pub const SYSCALL_ACCEPT: SysCallNumber = 0x2B;
pub const SYSCALL_SENDTO: SysCallNumber = 0x2C;
pub const SYSCALL_RECVFROM: SysCallNumber = 0x2D;
pub const SYSCALL_BIND: SysCallNumber = 0x31;
pub const SYSCALL_LISTEN: SysCallNumber = 0x32;
