//!
//! System Call Number List
//!

pub type SysCallNumber = u64;

pub const SYSCALL_EXIT: SysCallNumber = 0x3C;
pub const SYSCALL_WRITE: SysCallNumber = 0x01;
pub const SYSCALL_WRITEV: SysCallNumber = 0x14;
pub const SYSCALL_ARCH_PRCTL: SysCallNumber = 0x9E;
