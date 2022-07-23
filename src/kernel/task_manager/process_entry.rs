//!
//! Task Manager Process Entry
//!
//! This entry contains at least one thread entry.

use super::{ProcessStatus, TaskError, TaskSignal, ThreadEntry};

use crate::kernel::collections::init_struct;
use crate::kernel::collections::ptr_linked_list::{
    offset_of_list_node, PtrLinkedList, PtrLinkedListNode,
};
use crate::kernel::file_manager::File;
use crate::kernel::memory_manager::MemoryManager;
use crate::kernel::sync::spin_lock::{Mutex, SpinLockFlag};

use alloc::sync::Arc;
use alloc::vec::Vec;

#[allow(dead_code)]
pub struct ProcessEntry {
    pub(super) p_list: PtrLinkedListNode<Self>,
    pub(super) children: PtrLinkedList<Self>,
    pub(super) siblings: PtrLinkedListNode<Self>,
    pub(super) lock: SpinLockFlag,

    thread: PtrLinkedList<ThreadEntry>,
    signal: TaskSignal,
    status: ProcessStatus,
    memory_manager: *mut MemoryManager,
    process_id: usize,
    parent: *mut ProcessEntry, /* kernel process has invalid pointer */
    num_of_thread: usize,
    single_thread: Option<*mut ThreadEntry>,
    privilege_level: u8,
    next_thread_id: usize,

    files: Vec<Arc<Mutex<File<'static>>>>,
    file_vec_lock: SpinLockFlag,
}

impl ProcessEntry {
    pub const PROCESS_ENTRY_ALIGN: usize = 0;

    fn new() -> Self {
        Self {
            p_list: PtrLinkedListNode::new(),
            children: PtrLinkedList::new(),
            siblings: PtrLinkedListNode::new(),
            lock: SpinLockFlag::new(),
            thread: PtrLinkedList::new(),
            signal: TaskSignal::Normal,
            status: ProcessStatus::New,
            memory_manager: core::ptr::null_mut(),
            process_id: 0,
            parent: core::ptr::null_mut(),
            num_of_thread: 0,
            single_thread: None,
            privilege_level: 0,
            next_thread_id: 0,
            files: Vec::new(),
            file_vec_lock: SpinLockFlag::new(),
        }
    }

    /// Init ProcessEntry and set ThreadEntries to `Self::thread`.
    ///
    /// **`threads` must be unlocked.**
    pub fn init(
        &mut self,
        p_id: usize,
        parent: *mut Self,
        threads: &mut [&mut ThreadEntry],
        memory_manager: *mut MemoryManager,
        privilege_level: u8,
    ) {
        init_struct!(*self, Self::new());
        self.parent = parent;
        self.process_id = p_id;
        self.privilege_level = privilege_level;
        self.memory_manager = memory_manager;
        self.num_of_thread = threads.len();
        self.next_thread_id = 1;
        let _lock = self.lock.lock();

        if threads.len() == 1 {
            let _thread_lock = threads[0].lock.lock();
            threads[0].set_process(self as *mut _);
            threads[0].set_t_id(1);
            self.single_thread = Some(threads[0] as *mut _);
            self.update_next_thread_id();
        } else {
            self.single_thread = None;
            for i in 0..threads.len() {
                let _thread_lock = threads[0].lock.lock();
                threads[i].set_process(self as *mut _);
                threads[i].set_t_id(self.next_thread_id);
                drop(_thread_lock);
                self.update_next_thread_id();
                let thread = unsafe { &mut *(threads[i] as *mut ThreadEntry) };
                let prev = if i > 0 {
                    Some(unsafe { &mut *(threads[i - 1] as *mut ThreadEntry) })
                } else {
                    None
                };

                self.set_thread_into_thread_list(thread, prev)
                    .expect("Cannot insert thread.");
            }
        }
    }

    /// Chain `thread` into self.thread(List, ThreadEntry::t_list)
    ///
    /// This function does not check [Self::num_of_threads].
    /// [Self::lock] must be locked.
    fn set_thread_into_thread_list(
        &mut self,
        thread: &mut ThreadEntry,
        prev_thread: Option<&mut ThreadEntry>,
    ) -> Result<(), TaskError> {
        assert!(self.lock.is_locked());
        if self.thread.is_empty() {
            self.thread.insert_head(&mut thread.t_list);
        } else if let Some(prev_thread) = prev_thread {
            let _lock = thread.lock.lock();
            let _prev_lock = prev_thread
                .lock
                .try_lock()
                .or(Err(TaskError::ThreadLockError))?;
            self.thread
                .insert_after(&mut prev_thread.t_list, &mut thread.t_list);
        } else {
            /* Current chain the last of t_list */
            let tail = unsafe {
                self.thread
                    .get_last_entry_mut(offset_of_list_node!(ThreadEntry, t_list))
                    .unwrap()
            };
            let _lock = thread.lock.lock();
            let _prev_lock = tail.lock.try_lock().or(Err(TaskError::ThreadLockError))?;
            self.thread.insert_tail(&mut thread.t_list);
        }
        return Ok(());
    }

    fn update_next_thread_id(&mut self) {
        self.next_thread_id += 1;
    }

    pub const fn get_pid(&self) -> usize {
        self.process_id
    }

    pub const fn get_privilege_level(&self) -> u8 {
        self.privilege_level
    }

    pub const fn get_parent_process(&self) -> *mut Self {
        self.parent
    }

    pub fn get_memory_manager(&self) -> *mut MemoryManager {
        let _lock = self.lock.lock();
        let m = self.memory_manager;
        drop(_lock);
        return m;
    }

    /// Search the thread from [Self::thread]
    ///
    /// This function searches the thread having specified t_id.
    /// [Self::lock] must be locked.
    pub fn get_thread_mut(&mut self, t_id: usize) -> Option<&mut ThreadEntry> {
        assert!(self.lock.is_locked());
        if self.num_of_thread == 0 {
            return None;
        }
        if let Some(single_thread) = self.single_thread {
            let s_t = unsafe { &mut *single_thread };
            if s_t.get_t_id() == t_id {
                Some(s_t)
            } else {
                None
            }
        } else {
            for thread in unsafe {
                self.thread
                    .iter_mut(offset_of_list_node!(ThreadEntry, t_list))
            } {
                if thread.get_t_id() == t_id {
                    return Some(thread);
                }
            }
            None
        }
    }

    /// Add thread into ThreadList.
    ///
    /// This function adds `thread` into [Self::thread] or [Self::single_thread].
    /// [Self::lock] must be locked and `thread` must be unlocked.
    pub fn add_thread(&mut self, thread: &mut ThreadEntry) -> Result<(), TaskError> {
        assert!(self.lock.is_locked());
        assert!(!thread.lock.is_locked());

        thread.set_process(self as *mut _);
        thread.set_t_id(self.next_thread_id);
        self.update_next_thread_id();
        if self.num_of_thread == 0 {
            assert!(self.thread.is_empty());
            assert!(self.single_thread.is_none());
            self.single_thread = Some(thread as *mut _)
        } else if self.num_of_thread == 1 {
            assert!(self.thread.is_empty());
            assert!(self.single_thread.is_some());
            let single_thread = unsafe { &mut *self.single_thread.take().unwrap() };
            self.set_thread_into_thread_list(single_thread, None)?;
            self.set_thread_into_thread_list(thread, None /* compare and set */)?;
        } else {
            assert!(!self.thread.is_empty());
            assert!(self.single_thread.is_none());
            self.set_thread_into_thread_list(thread, None)?;
        }
        self.num_of_thread += 1;
        return Ok(());
    }

    /// Remove `thread` from ThreadList.
    ///
    /// This function removes thread from [Self::t_list] and adjust.
    /// [Self::lock] must be locked, and `thread` must be unlocked.
    pub fn remove_thread(&mut self, thread: &mut ThreadEntry) -> Result<(), TaskError> {
        assert!(self.lock.is_locked());
        assert!(!thread.lock.is_locked());

        if self.num_of_thread == 0 {
            return Err(TaskError::InvalidProcessEntry);
        } else if self.num_of_thread == 1 {
            assert!(self.thread.is_empty());
            assert!(self.single_thread.is_some());
            self.single_thread = None;
        } else if self.num_of_thread == 2 {
            self.thread.remove(&mut thread.t_list);
            let single_thread = unsafe {
                self.thread
                    .take_first_entry(offset_of_list_node!(ThreadEntry, t_list))
                    .unwrap()
            };
            assert!(self.thread.is_empty());
            self.single_thread = Some(single_thread as *mut _);
        } else {
            self.thread.remove(&mut thread.t_list);
        }
        self.num_of_thread -= 1;
        return Ok(());
    }

    pub fn take_thread(&mut self) -> Result<Option<&mut ThreadEntry>, TaskError> {
        assert!(self.lock.is_locked());
        if self.num_of_thread == 0 {
            Ok(None)
        } else if self.num_of_thread == 1 {
            let single = unsafe { &mut *self.single_thread.unwrap() };
            let _lock = single.lock.lock();
            self.remove_thread(single)?;
            drop(_lock);
            Ok(Some(single))
        } else {
            let thread = unsafe {
                self.thread
                    .get_first_entry_mut(offset_of_list_node!(ThreadEntry, t_list))
                    .unwrap()
            };
            let _lock = thread.lock.lock();
            self.remove_thread(thread)?;
            drop(_lock);
            Ok(Some(thread))
        }
    }

    pub fn get_file(&self, index: usize) -> Option<Arc<Mutex<File<'static>>>> {
        let _lock = if self.num_of_thread == 1 {
            None
        } else {
            Some(self.file_vec_lock.lock())
        };
        let result = self.files.get(index).and_then(|f| Some(f.clone()));
        drop(_lock);
        result
    }

    pub fn add_file(&mut self, f: File<'static>) -> usize {
        let _lock = if self.num_of_thread == 1 {
            None
        } else {
            Some(self.file_vec_lock.lock())
        };
        let i = self.files.len();
        self.files.push(Arc::new(Mutex::new(f)));
        drop(_lock);
        i
    }
}
