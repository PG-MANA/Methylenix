//!
//! Task Manager Process Entry
//!
//! This entry contains at least one thread entry.

use super::{ProcessStatus, TaskError, TaskSignal, ThreadEntry};

use crate::kernel::collections::init_struct;
use crate::kernel::collections::ptr_linked_list::{PtrLinkedList, PtrLinkedListNode};
use crate::kernel::file_manager::{File, PathInfo};
use crate::kernel::memory_manager::{
    MemoryError, MemoryManager,
    data_type::{Address, MSize, VAddress},
};
use crate::kernel::sync::spin_lock::{IrqSaveSpinLockFlag, Mutex};

use core::mem::offset_of;
use core::ptr::NonNull;

use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;

pub struct ProcessEntry {
    pub(super) p_list: PtrLinkedListNode<Self>,
    pub(super) children: PtrLinkedList<Self>,
    pub(super) siblings: PtrLinkedListNode<Self>,
    pub(super) lock: IrqSaveSpinLockFlag,

    thread: PtrLinkedList<ThreadEntry>,
    signal: TaskSignal,
    status: ProcessStatus,
    memory_manager: *mut MemoryManager,
    heap_area: (
        VAddress,
        MSize, /* mapped size */
        MSize, /* using size */
    ),
    directory: Option<DirectoryInfo>,
    process_id: usize,
    parent: Option<NonNull<ProcessEntry>>,
    num_of_thread: usize,
    privilege_level: u8,
    next_thread_id: usize,
    files: Mutex<Vec<Arc<Mutex<File>>>>,
}

struct DirectoryInfo {
    cwd: String,
    cwd_file: File,
}

impl ProcessEntry {
    const fn new() -> Self {
        Self {
            p_list: PtrLinkedListNode::new(),
            children: PtrLinkedList::new(),
            siblings: PtrLinkedListNode::new(),
            lock: IrqSaveSpinLockFlag::new(),
            thread: PtrLinkedList::new(),
            signal: TaskSignal::Normal,
            status: ProcessStatus::New,
            memory_manager: core::ptr::null_mut(),
            heap_area: (VAddress::new(0), MSize::new(0), MSize::new(0)),
            directory: None,
            process_id: 0,
            parent: None,
            num_of_thread: 0,
            privilege_level: 0,
            next_thread_id: 0,
            files: Mutex::new(Vec::new()),
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
        self.parent = NonNull::new(parent);
        self.process_id = p_id;
        self.privilege_level = privilege_level;
        self.memory_manager = memory_manager;
        self.num_of_thread = threads.len();
        self.next_thread_id = 1;
        let _lock = self.lock.lock();

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

    pub fn init_heap_size(&mut self, heap_start: VAddress, heap_size: MSize) {
        let _lock = self.lock.lock();
        assert_eq!(
            self.heap_area,
            (VAddress::new(0), MSize::new(0), MSize::new(0))
        );
        self.heap_area = (heap_start, heap_size, MSize::new(0));
    }

    pub fn get_current_directory_name(&self) -> Option<String> {
        let _lock = self.lock.lock();
        self.directory.as_ref().map(|d| d.cwd.clone())
    }

    pub fn set_current_directory(&mut self, path: &PathInfo, file: File) {
        let _lock = self.lock.lock();
        self.directory = Some(DirectoryInfo {
            cwd: path.as_str().to_string(),
            cwd_file: file,
        });
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
            unsafe { self.thread.insert_head(&mut thread.t_list) };
        } else if let Some(prev_thread) = prev_thread {
            let _lock = thread.lock.lock();
            let _prev_lock = prev_thread
                .lock
                .try_lock()
                .or(Err(TaskError::ThreadLockError))?;
            unsafe {
                self.thread
                    .insert_after(&mut prev_thread.t_list, &mut thread.t_list)
            };
        } else {
            /* Current chain the last of t_list */
            let tail = self
                .thread
                .get_last_entry_mut(offset_of!(ThreadEntry, t_list))
                .map(|t| unsafe { &mut *t })
                .unwrap();
            let _lock = thread.lock.lock();
            let _prev_lock = tail.lock.try_lock().or(Err(TaskError::ThreadLockError))?;
            unsafe { self.thread.insert_tail(&mut thread.t_list) };
        }
        Ok(())
    }

    fn update_next_thread_id(&mut self) {
        self.next_thread_id += 1;
    }

    pub const fn get_process_status(&self) -> ProcessStatus {
        self.status
    }

    pub fn set_process_status(&mut self, status: ProcessStatus) {
        let _lock = self.lock.lock();
        self.status = status;
    }

    pub const fn get_pid(&self) -> usize {
        self.process_id
    }

    pub const fn get_privilege_level(&self) -> u8 {
        self.privilege_level
    }

    pub const fn get_parent_process(&self) -> Option<NonNull<Self>> {
        self.parent
    }

    pub fn get_memory_manager(&self) -> *mut MemoryManager {
        let _lock = self.lock.lock();
        let m = self.memory_manager;
        drop(_lock);
        m
    }

    pub fn get_heap_area(&self) -> (VAddress, MSize) {
        let _lock = self.lock.lock();
        (self.heap_area.0, self.heap_area.1)
    }

    pub fn increate_heap(&mut self, new_address: VAddress) -> Result<VAddress, TaskError> {
        let _lock = self.lock.lock();
        if self.heap_area.0.is_zero() {
            return Err(TaskError::InvalidProcessEntry);
        } else if new_address.is_zero() {
            return Ok(self.heap_area.0);
        } else if new_address < self.heap_area.0 {
            return Err(TaskError::MemoryError(MemoryError::InvalidAddress));
        }

        let new_heap_size = new_address - self.heap_area.0;
        let needed_heap_size = new_heap_size + self.heap_area.2;
        if needed_heap_size <= self.heap_area.1 {
            self.heap_area.2 = needed_heap_size;
            return Ok(new_address);
        }

        let memory_manager = unsafe { &mut *self.memory_manager };
        assert!(!memory_manager.is_kernel_memory_manager());
        let new_map_size = needed_heap_size.page_align_up();
        assert!(new_map_size > self.heap_area.1);
        let result = memory_manager.mremap(self.heap_area.0, self.heap_area.1, new_map_size);
        match result {
            Ok(_) => {
                assert_eq!(self.heap_area.0 + new_map_size, new_address);
                self.heap_area.1 = new_map_size;
                Ok(new_address)
            }
            Err(e) => Err(TaskError::MemoryError(e)),
        }
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
        unsafe { self.thread.iter_mut(offset_of!(ThreadEntry, t_list)) }
            .find(|t| t.get_t_id() == t_id)
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
        self.set_thread_into_thread_list(thread, None)?;
        self.num_of_thread += 1;
        Ok(())
    }

    /// Remove `thread` from ThreadList.
    ///
    /// This function removes thread from [Self::t_list] and adjusts the list.
    /// [Self::lock] must be locked, and `thread` must be unlocked.
    pub fn remove_thread(&mut self, thread: &mut ThreadEntry) -> Result<(), TaskError> {
        assert!(self.lock.is_locked());
        assert!(!thread.lock.is_locked());

        if self.num_of_thread == 0 {
            return Err(TaskError::InvalidProcessEntry);
        }
        unsafe { self.thread.remove(&mut thread.t_list) };
        self.num_of_thread -= 1;
        Ok(())
    }

    pub fn take_thread(&mut self) -> Result<Option<&mut ThreadEntry>, TaskError> {
        assert!(self.lock.is_locked());
        match self
            .thread
            .get_first_entry_mut(offset_of!(ThreadEntry, t_list))
        {
            Some(t) => {
                let t = unsafe { &mut *t };
                self.remove_thread(t)?;
                Ok(Some(t))
            }
            None => {
                assert_eq!(self.num_of_thread, 0);
                Ok(None)
            }
        }
    }

    pub fn get_file(&self, index: usize) -> Option<Arc<Mutex<File>>> {
        self.files.lock().unwrap().get(index).cloned()
    }

    pub fn add_file(&mut self, f: File) -> usize {
        let mut l = self.files.lock().unwrap();
        l.push(Arc::new(Mutex::new(f)));
        l.len() - 1
    }

    pub fn remove_file_from_list(&mut self, index: usize) -> Result<Arc<Mutex<File>>, ()> {
        let mut l = self.files.lock()?;
        if index < l.len() {
            let file = core::mem::replace(&mut l[index], Arc::new(Mutex::new(File::invalid())));
            Ok(file)
        } else {
            Err(())
        }
    }
}
