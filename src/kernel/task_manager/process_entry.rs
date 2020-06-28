/*
 * Task Manager Process Entry
 * This entry contains at least one thread entry
 */

use super::{TaskSignal, TaskStatus, ThreadEntry};

use kernel::memory_manager::MemoryManager;
use kernel::ptr_linked_list::{PtrLinkedList, PtrLinkedListNode};
use kernel::sync::spin_lock::SpinLockFlag;

use core::ptr::NonNull;

pub struct ProcessEntry {
    lock: SpinLockFlag,
    p_list: PtrLinkedListNode<Self>,
    signal: TaskSignal,
    status: TaskStatus,
    memory_manager: NonNull<MemoryManager>,
    process_id: usize,
    thread: PtrLinkedList<ThreadEntry>,
    single_thread: Option<*mut ThreadEntry>,
    privilege_level: u8,
    priority_level: i8,
    num_of_thread: usize,
}

impl ProcessEntry {
    pub const PROCESS_ENTRY_ALIGN_ORDER: usize = 0;

    pub fn init(
        &mut self,
        process_id: usize,
        thread: *mut ThreadEntry,
        privilege_level: u8,
        priority_level: i8,
    ) {
        self.lock = SpinLockFlag::new();
        let _lock = self.lock.lock();
        self.p_list = PtrLinkedListNode::new();
        self.signal = TaskSignal::Normal;
        self.status = TaskStatus::New;
        self.process_id = process_id;
        self.thread = PtrLinkedList::new();
        self.single_thread = Some(thread);
        self.privilege_level = privilege_level;
        self.priority_level = priority_level;
        self.num_of_thread = 1;
    }

    pub fn add_thread(&mut self, thread: &mut ThreadEntry) {
        let _lock = self.lock.lock();
        if self.num_of_thread == 0 {
            assert!(self.thread.get_first_entry_as_ptr().is_none());
            assert!(self.single_thread.is_none());
            self.num_of_thread = 1;
            self.single_thread = Some(thread as *mut _);
        } else if self.num_of_thread == 1 {
            assert!(self.thread.get_first_entry_as_ptr().is_none());
            assert!(self.single_thread.is_some());
            let old_thread = self.single_thread.unwrap();
            self.single_thread = None;
            unsafe { &mut *old_thread }.set_up_to_be_root_of_p_list(&mut self.thread);
            unsafe { &mut *old_thread }.insert_after_of_p_list(thread);
        } else {
            assert!(self.thread.get_first_entry_as_ptr().is_none());
            assert!(self.single_thread.is_none());
            let old_entry = unsafe { self.thread.get_first_entry_mut().unwrap() };
            thread.set_up_to_be_root_of_p_list(&mut self.thread);
            thread.insert_after_of_p_list(old_entry);
        }
        thread.set_process(self as *mut _);
    }
}
