//!
//! Task Wait Queue Manager
//!
//! This module manages a list of sleeping tasks.
//! This will be used by the device handlers.
//! Device handlers contain this manager, and when data is arrived, they search the thread to wakeup
//! from this manager.
//!
//! Do not call this Manager in interrupt handlers, please add work_queue and call from there.

use super::{TaskError, TaskStatus, ThreadEntry};

use crate::arch::target_arch::device::cpu::is_interrupt_enabled;
use crate::arch::target_arch::interrupt::InterruptManager;

use crate::kernel::collections::ptr_linked_list::PtrLinkedList;
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::sync::spin_lock::SpinLockFlag;

use core::mem::offset_of;

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

    fn _add_thread(&mut self, thread: &mut ThreadEntry) -> Result<(), TaskError> {
        assert!(thread.lock.is_locked());

        if let Some(last_thread) = self
            .list
            .get_last_entry_mut(offset_of!(ThreadEntry, sleep_list))
            .map(|t| unsafe { &mut *t })
        {
            let _last_thread_lock = last_thread
                .lock
                .try_lock()
                .or(Err(TaskError::ThreadLockError))?;
            unsafe { self.list.insert_tail(&mut thread.sleep_list) };
        } else {
            unsafe { self.list.insert_head(&mut thread.sleep_list) }
        }
        Ok(())
    }

    /// Add the thread to WaitQueue.
    ///
    /// `thread` must be unlocked.
    pub fn add_thread(&mut self, thread: &mut ThreadEntry) -> Result<(), TaskError> {
        assert!(!thread.lock.is_locked());
        let _lock = self.lock.lock();
        let _thread_lock = thread.lock.try_lock().or(Err(TaskError::ThreadLockError))?;
        self._add_thread(thread)
    }

    pub fn add_current_thread(&mut self) -> Result<(), TaskError> {
        assert!(is_interrupt_enabled());
        let _lock = self.lock.lock();

        /* Chain running_thread.sleep_list */
        let interrupt_flag = InterruptManager::save_and_disable_local_irq();
        let running_thread = get_cpu_manager_cluster().run_queue.get_running_thread();
        let result: Result<(), TaskError> = try {
            let _running_thread_lock = running_thread
                .lock
                .try_lock()
                .or(Err(TaskError::ThreadLockError))?;
            self._add_thread(running_thread)?
        };
        if result.is_ok() {
            drop(_lock);
            get_cpu_manager_cluster()
                .run_queue
                .sleep_current_thread(Some(interrupt_flag), TaskStatus::Interruptible)?;
        } else {
            InterruptManager::restore_local_irq(interrupt_flag);
        }
        result
    }

    pub fn wakeup_one(&mut self) -> Result<(), TaskError> {
        let _lock = self.lock.lock();
        if let Some(thread) = self
            .list
            .get_first_entry_mut(offset_of!(ThreadEntry, sleep_list))
            .map(|t| unsafe { &mut *t })
        {
            let _thread_lock = thread.lock.lock();
            unsafe { self.list.remove(&mut thread.sleep_list) };
            drop(_thread_lock);
            get_kernel_manager_cluster()
                .task_manager
                .wake_up_thread(thread)?;
            Ok(())
        } else {
            Err(TaskError::InvalidThreadEntry)
        }
    }

    pub fn wakeup_all(&mut self) -> Result<(), TaskError> {
        let _lock = self.lock.lock();
        let mut cursor = unsafe {
            self.list
                .cursor_front_mut(offset_of!(ThreadEntry, sleep_list))
        };
        while let Some(thread) = cursor.current().map(|t| unsafe { &mut *t }) {
            let _thread_lock = thread.lock.lock();
            unsafe { cursor.remove_current() };
            drop(_thread_lock);
            get_kernel_manager_cluster()
                .task_manager
                .wake_up_thread(thread)?;
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        let _lock = self.lock.lock();
        let result = self.list.is_empty();
        drop(_lock);
        result
    }
}
