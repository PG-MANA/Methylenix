//!
//! Task Wait Queue Manager
//!
//! This module manages a list of sleeping tasks.
//! This will be used by the device handlers.

use super::ThreadEntry;

use crate::kernel::ptr_linked_list::PtrLinkedList;
use crate::kernel::sync::spin_lock::SpinLockFlag;

pub struct WaitQueue {
    lock: SpinLockFlag,
    list: PtrLinkedList<ThreadEntry>,
}
