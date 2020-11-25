//!
//! Task Manager Process Entry
//!
//! This entry contains at least one thread entry

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
    children: PtrLinkedList<ThreadEntry>,
    siblings: PtrLinkedListNode<Self>,
    thread: PtrLinkedList<ThreadEntry>,
    num_of_thread: usize,
    single_thread: Option<*mut ThreadEntry>,
    privilege_level: u8,
    priority_level: i8,
    next_thread_id: usize,
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
        self.next_thread_id = 1;
        self.children = PtrLinkedList::new();
        self.siblings = PtrLinkedListNode::new();

        self.thread = PtrLinkedList::new();
        if threads.len() == 1 {
            threads[0].set_process(self as *mut _);
            threads[0].set_t_id(1);
            self.single_thread = Some(threads[0] as *mut _);
            self.update_next_thread_id();
        } else {
            self.single_thread = None;
            threads[0].set_up_to_be_root_of_p_list(&mut self.thread);
            threads[0].set_process(self as *mut _);
            threads[0].set_t_id(self.next_thread_id);
            self.update_next_thread_id();
            for i in 1..threads.len() {
                let pointer = threads[i] as *mut ThreadEntry;
                threads[i - 1].insert_after_of_p_list(unsafe { &mut *pointer });
                threads[i].set_process(self as *mut _);
                threads[i].set_t_id(self.next_thread_id);
                self.update_next_thread_id();
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

    fn update_next_thread_id(&mut self) {
        self.next_thread_id += 1;
    }

    pub const fn get_pid(&self) -> usize {
        self.process_id
    }

    pub fn get_thread(&mut self, t_id: usize) -> Option<&mut ThreadEntry> {
        if let Some(single_thread) = self.single_thread {
            let s_t = unsafe { &mut *single_thread };
            if s_t.get_t_id() == t_id {
                Some(s_t)
            } else {
                None
            }
        } else {
            for e in self.thread.iter_mut() {
                let thread = unsafe { &mut *e };
                if thread.get_t_id() == t_id {
                    return Some(thread);
                }
            }
            None
        }
    }

    pub fn add_thread(&mut self, thread: &mut ThreadEntry) {
        let _lock = self.lock.lock();
        if self.num_of_thread == 0 {
            assert!(self.thread.get_first_entry_as_ptr().is_none());
            assert!(self.single_thread.is_none());
            self.single_thread = Some(thread as *mut _);
        } else if self.num_of_thread == 1 {
            assert!(self.thread.get_first_entry_as_ptr().is_none());
            assert!(self.single_thread.is_some());
            let old_thread = self.single_thread.unwrap();
            self.single_thread = None;
            unsafe { &mut *old_thread }.set_up_to_be_root_of_p_list(&mut self.thread);
            unsafe { &mut *old_thread }.insert_after_of_p_list(thread);
        } else {
            assert!(self.thread.get_first_entry_as_ptr().is_some());
            assert!(self.single_thread.is_none());
            unsafe { self.thread.get_last_entry_mut().unwrap() }.insert_after_of_p_list(thread)
        }
        thread.set_process(self as *mut _);
        thread.set_t_id(self.next_thread_id);
        self.update_next_thread_id();
        self.num_of_thread += 1;
    }

    pub fn get_next_process_from_p_list_mut(&mut self) -> Option<*mut Self> {
        self.p_list.get_next_mut_as_ptr()
    }
}
