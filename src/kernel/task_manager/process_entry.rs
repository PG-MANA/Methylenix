/*
 * Task Manager Process Entry
 * This entry contains at least one thread entry
 */

use super::{ProcessStatus, TaskSignal, ThreadEntry};

use crate::kernel::memory_manager::MemoryManager;
use crate::kernel::ptr_linked_list::{PtrLinkedList, PtrLinkedListNode};
use crate::kernel::sync::spin_lock::{Mutex, SpinLockFlag};

pub struct ProcessEntry {
    lock: SpinLockFlag,
    p_list: PtrLinkedListNode<Self>,
    signal: TaskSignal,
    status: ProcessStatus,
    memory_manager: *const Mutex<MemoryManager>,
    process_id: usize,
    parent: *mut ProcessEntry, /* kernel process has invalid pointer */
    thread: PtrLinkedList<ThreadEntry>,
    num_of_thread: usize,
    single_thread: Option<*mut ThreadEntry>,
    privilege_level: u8,
    priority_level: i8,
}

impl ProcessEntry {
    pub const PROCESS_ENTRY_ALIGN_ORDER: usize = 0;

    #[allow(dead_code)]
    pub fn init(
        &mut self,
        p_id: usize,
        parent: *mut Self,
        threads: &mut [&mut ThreadEntry],
        memory_manager: *const Mutex<MemoryManager>,
        privilege_level: u8,
        priority_level: i8,
    ) {
        self.lock = SpinLockFlag::new();
        let _lock = self.lock.lock();
        self.parent = parent;
        self._init(
            p_id,
            threads,
            memory_manager,
            privilege_level,
            priority_level,
        );
    }

    pub fn _init(
        &mut self,
        p_id: usize,
        threads: &mut [&mut ThreadEntry],
        memory_manager: *const Mutex<MemoryManager>,
        privilege_level: u8,
        priority_level: i8,
    ) {
        /* assume be locked */
        assert_ne!(threads.len(), 0);
        self.p_list = PtrLinkedListNode::new();
        self.signal = TaskSignal::Normal;
        self.status = ProcessStatus::Normal;
        self.process_id = p_id;
        self.privilege_level = privilege_level;
        self.priority_level = priority_level;
        self.memory_manager = memory_manager;
        self.num_of_thread = threads.len();

        self.thread = PtrLinkedList::new();
        if threads.len() == 1 {
            threads[0].set_process(self as *mut _);
            self.single_thread = Some(threads[0] as *mut _);
        } else {
            self.single_thread = None;
            threads[0].set_up_to_be_root_of_p_list(&mut self.thread);
            threads[0].set_process(self as *mut _);
            for i in 1..threads.len() {
                let pointer = threads[i] as *mut ThreadEntry;
                threads[i - 1].insert_after_of_p_list(unsafe { &mut *pointer });
                threads[i - 1].set_process(self as *mut _);
            }
        }
    }

    pub fn init_kernel_process(
        &mut self,
        threads: &mut [&mut ThreadEntry],
        memory_manager: *const Mutex<MemoryManager>,
        priority_level: i8,
    ) {
        self.lock = SpinLockFlag::new();
        let _lock = self.lock.lock();
        self._init(0, threads, memory_manager, 0, priority_level);
    }

    pub const fn get_pid(&self) -> usize {
        self.process_id
    }

    #[allow(dead_code)]
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
