//!
//! System Call Number List for x86_64
//!
//! System call numbers are compatible with Linux
//!

pub type SysCallNumber = u64;

pub const SYSCALL_EXIT: SysCallNumber = 60;
pub const SYSCALL_EXIT_GROUP: SysCallNumber = 231;

pub const SYSCALL_OPEN: SysCallNumber = 2;
pub const SYSCALL_CLOSE: SysCallNumber = 3;
pub const SYSCALL_LSEEK: SysCallNumber = 8;

pub const SYSCALL_READ: SysCallNumber = 0;
pub const SYSCALL_WRITE: SysCallNumber = 1;
pub const SYSCALL_WRITEV: SysCallNumber = 20;

pub const SYSCALL_SET_TID_ADDRESS: SysCallNumber = 218;
pub const SYSCALL_ARCH_PRCTL: SysCallNumber = 158;

pub const SYSCALL_BRK: SysCallNumber = 12;
pub const SYSCALL_MMAP: SysCallNumber = 9;
pub const SYSCALL_MUNMAP: SysCallNumber = 11;

pub const SYSCALL_SOCKET: SysCallNumber = 41;
pub const SYSCALL_ACCEPT: SysCallNumber = 43;
pub const SYSCALL_SENDTO: SysCallNumber = 44;
pub const SYSCALL_RECVFROM: SysCallNumber = 45;
pub const SYSCALL_BIND: SysCallNumber = 49;
pub const SYSCALL_LISTEN: SysCallNumber = 50;
