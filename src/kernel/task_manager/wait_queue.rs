//!
//! Task Wait Queue Manager
//!
//! This module manages a list of sleeping tasks.
//! This will be used by the device handlers.
//! Device hadnlers contains this manager and when data is arrived, they search the thread to wakeup
//! from this manager.

use super::ThreadEntry;

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::ptr_linked_list::PtrLinkedList;
use crate::kernel::sync::spin_lock::SpinLockFlag;

pub struct WaitQueue {
    lock: SpinLockFlag,
    list: PtrLinkedList<ThreadEntry>,
}

impl WaitQueue {
    pub const fn new() -> Self {
        Self {
            lock: SpinLockFlag::new(),
            list: PtrLinkedList::new(),
        }
    }

    pub fn add_thread(&mut self, thread: &mut ThreadEntry) {
        let _lock = self.lock.lock();
        if let Some(first_thread) = unsafe { self.list.get_first_entry_mut() } {
            let _first_thread_lock = first_thread.lock.lock();
            let _thread_lock = thread.lock.lock();
            let ptr = thread as *mut _;
            thread.sleep_list.set_ptr(ptr);
            first_thread.sleep_list.insert_after(&mut thread.sleep_list);
        } else {
            let _thread_lock = thread.lock.lock();
            thread.set_up_to_be_root_of_sleep_list(&mut self.list);
        }
    }

    pub fn wakeup(&mut self) {
        let _lock = self.lock.lock();
        for t in self.list.iter_mut() {
            let thread = unsafe { &mut *t };
            get_kernel_manager_cluster()
                .task_manager
                .wakeup_target_thread(thread);
        }
    }

    pub fn remove_all_threads(&mut self) {
        let _lock = self.lock.lock();
        if let Some(t_ptr) = self.list.get_first_entry_mut_as_ptr() {
            let mut t = unsafe { &mut *t_ptr };
            loop {
                let _t_lock = t.lock.lock();
                let next = unsafe { t.sleep_list.get_next_mut() };
                t.sleep_list.remove_from_list(&mut self.list);
                if next.is_none() {
                    return;
                }
                t = next.unwrap();
            }
        }
    }
}
