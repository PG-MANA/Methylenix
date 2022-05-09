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
