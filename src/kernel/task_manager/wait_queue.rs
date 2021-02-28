//!
//! Task Wait Queue Manager
//!
//! This module manages a list of sleeping tasks.
//! This will be used by the device handlers.
//! Device handlers contains this manager and when data is arrived, they search the thread to wakeup
//! from this manager.
//!
//! Do not call this Manager in interrupt handlers, please add work_queue and call from there.

use super::{TaskError, TaskStatus, ThreadEntry};

use crate::arch::target_arch::interrupt::InterruptManager;
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
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

    pub fn _add_thread(&mut self, thread: &mut ThreadEntry) -> Result<(), TaskError> {
        assert!(!thread.lock.is_locked());

        if let Some(first_thread) = unsafe { self.list.get_first_entry_mut() } {
            let _first_thread_lock = first_thread
                .lock
                .try_lock()
                .or(Err(TaskError::ThreadLockError))?;
            let _thread_lock = thread.lock.lock();
            thread.set_ptr_to_list();
            thread.sleep_list.unset_prev_and_next();
            first_thread.sleep_list.insert_after(&mut thread.sleep_list);
        } else {
            let _thread_lock = thread.lock.lock();
            thread.sleep_list.unset_prev_and_next();
            self.list
                .set_first_entry(Some(&mut thread.sleep_list as *mut _));
        }
        return Ok(());
    }

    /// Add the thread to WaitQueue.
    ///
    /// `thread` must be unlocked.
    pub fn add_thread(&mut self, thread: &mut ThreadEntry) -> Result<(), TaskError> {
        assert!(!thread.lock.is_locked());
        let _lock = self.lock.lock();
        self._add_thread(thread)
    }

    pub fn add_current_thread(&mut self) -> Result<(), TaskError> {
        let _lock = self.lock.lock();

        /* Chain running_thread.sleep_list */
        let interrupt_flag = InterruptManager::save_and_disable_local_irq();
        let running_thread = get_cpu_manager_cluster().run_queue.get_running_thread();
        let result: Result<(), TaskError> = try {
            let _running_thread_lock = running_thread
                .lock
                .try_lock()
                .or(Err(TaskError::ThreadLockError))?;
            running_thread.sleep_list.unset_prev_and_next();
            self._add_thread(running_thread)?
        };
        if result.is_ok() {
            get_cpu_manager_cluster()
                .run_queue
                .sleep_current_thread(Some(interrupt_flag))?;
        } else {
            InterruptManager::restore_local_irq(interrupt_flag);
        }
        return result;
    }

    pub fn wakeup(&mut self) -> Result<(), TaskError> {
        let _lock = self.lock.lock();
        for t in self.list.iter_mut() {
            let thread = unsafe { &mut *t };
            let _thread_lock = thread.lock.lock();
            thread.set_task_status(TaskStatus::CanRun);
            thread.sleep_list.remove_from_list(&mut self.list);
            thread.sleep_list.unset_prev_and_next();
            drop(_thread_lock);
            get_kernel_manager_cluster()
                .task_manager
                .wake_up_thread(thread)?;
        }
        return Ok(());
    }
}
