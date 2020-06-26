/*
 * Task Manager Thread Entry
 * This entry contains some arch-depending data
 */

use super::ProcessEntry;

use arch::target_arch::context::context_data::ContextData;

use kernel::ptr_linked_list::PtrLinkedListNode;
use kernel::sync::spin_lock::SpinLockFlag;

use core::ptr::NonNull;

pub struct ThreadEntry {
    lock: SpinLockFlag,
    p_list: PtrLinkedListNode<Self>,
    run_list: PtrLinkedListNode<Self>,
    signal: u8,
    thread_id: usize,
    process: NonNull<ProcessEntry>,
    context_data: ContextData,
}

impl ThreadEntry {
    pub const THREAD_ENTRY_ALIGN_ORDER: usize = 0;
}
