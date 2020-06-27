/*
 * Task Manager Thread Entry
 * This entry contains some arch-depending data
 */

use super::{ProcessEntry, TaskSignal, TaskStatus};

use arch::target_arch::context::context_data::ContextData;

use kernel::ptr_linked_list::{PtrLinkedList, PtrLinkedListNode};
use kernel::sync::spin_lock::SpinLockFlag;

use core::ptr::NonNull;

pub struct ThreadEntry {
    lock: SpinLockFlag,
    p_list: PtrLinkedListNode<Self>,
    run_list: PtrLinkedListNode<Self>,
    status: TaskStatus,
    thread_id: usize,
    process: NonNull<ProcessEntry>,
    context_data: ContextData,
    privilege_level: u8,
    priority_level: i8,
}

impl ThreadEntry {
    pub const THREAD_ENTRY_ALIGN_ORDER: usize = 0;

    pub fn init(
        &mut self,
        thread_id: usize,
        process: *mut ProcessEntry,
        privilege_level: u8,
        priority_level: i8,
        context_data: ContextData,
    ) {
        self.lock = SpinLockFlag::new();
        let _lock = self.lock.lock();
        self.p_list = PtrLinkedListNode::new();
        self.run_list = PtrLinkedListNode::new();
        self.status = TaskStatus::New;
        self.thread_id = thread_id;
        self.process = NonNull::new(process).unwrap();
        self.context_data = context_data;
        self.privilege_level = privilege_level;
        self.priority_level = priority_level;
    }

    pub fn set_up_to_be_root_of_p_list(&mut self, list_head: &mut PtrLinkedList<Self>) {
        let _lock = self.lock.lock();
        let ptr = self as *mut _;
        self.p_list.set_ptr(ptr);
        self.p_list.terminate_prev_entry();
        list_head.set_first_entry(&mut self.p_list);
    }

    pub fn insert_after_of_p_list(&mut self, entry: &mut Self) {
        let _lock = self.lock.lock();
        if entry.p_list.is_invalid_ptr() {
            let ptr = entry as *mut Self;
            entry.p_list.set_ptr(ptr);
        }
        let ptr = self as *mut _;
        self.p_list.set_ptr(ptr);
        self.p_list
            .insert_after(unsafe { &mut *(&mut entry.p_list as *mut _) }); //must fix
    }

    pub fn set_process(&mut self, process: *mut ProcessEntry) {
        self.process = NonNull::new(process).unwrap();
    }
}
