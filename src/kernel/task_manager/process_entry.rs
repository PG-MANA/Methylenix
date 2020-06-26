/*
 * Task Manager Process Entry
 * This entry contains at least one thread entry
 */

use super::ThreadEntry;

use kernel::memory_manager::MemoryManager;
use kernel::ptr_linked_list::{PtrLinkedList, PtrLinkedListNode};
use kernel::sync::spin_lock::SpinLockFlag;

use core::ptr::NonNull;

pub struct ProcessEntry {
    lock: SpinLockFlag,
    p_list: PtrLinkedListNode<Self>,
    signal: u8,
    memory_manager: NonNull<MemoryManager>,
    process_id: usize,
    thread: PtrLinkedList<ThreadEntry>,
    single_thread: Option<*mut ThreadEntry>,
}

impl ProcessEntry {
    pub const PROCESS_ENTRY_ALIGN_ORDER: usize = 0;
}
