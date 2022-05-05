//!
//! System Call Number List
//!

pub type SysCallNumber = u64;

pub const SYSCALL_EXIT: SysCallNumber = 0x01;
pub const SYSCALL_WRITE: SysCallNumber = 0x04;
