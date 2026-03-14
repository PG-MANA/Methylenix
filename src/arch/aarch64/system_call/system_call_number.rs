//!
//! System Call Number List for AArch64
//!
//! System call numbers are compatible with Linux
//!

pub type SysCallNumber = u64;

pub const SYSCALL_EXIT: SysCallNumber = 93;
pub const SYSCALL_EXIT_GROUP: SysCallNumber = 94;

pub const SYSCALL_OPEN: SysCallNumber = 56;
pub const SYSCALL_CLOSE: SysCallNumber = 57;
pub const SYSCALL_LSEEK: SysCallNumber = 62;

pub const SYSCALL_READ: SysCallNumber = 63;
pub const SYSCALL_WRITE: SysCallNumber = 64;
pub const SYSCALL_WRITEV: SysCallNumber = 66;

pub const SYSCALL_SET_TID_ADDRESS: SysCallNumber = 96;
pub const SYSCALL_ARCH_PRCTL: SysCallNumber = 167;

pub const SYSCALL_BRK: SysCallNumber = 214;
pub const SYSCALL_MMAP: SysCallNumber = 22;
pub const SYSCALL_MUNMAP: SysCallNumber = 215;

pub const SYSCALL_SOCKET: SysCallNumber = 198;
pub const SYSCALL_ACCEPT: SysCallNumber = 202;
pub const SYSCALL_SENDTO: SysCallNumber = 206;
pub const SYSCALL_RECVFROM: SysCallNumber = 207;
pub const SYSCALL_BIND: SysCallNumber = 200;
pub const SYSCALL_LISTEN: SysCallNumber = 201;
