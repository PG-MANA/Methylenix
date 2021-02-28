//!
//! Task Run Queue
//!
//! This module manages per-cpu run queue.

use super::thread_entry::ThreadEntry;
use super::{TaskError, TaskStatus};

use crate::arch::target_arch::context::context_data::ContextData;
use crate::arch::target_arch::device::cpu::is_interrupt_enabled;
use crate::arch::target_arch::interrupt::{InterruptManager, StoredIrqData};

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::ptr_linked_list::PtrLinkedList;
use crate::kernel::sync::spin_lock::SpinLockFlag;

pub struct RunQueue {
    lock: SpinLockFlag,
    run_list: PtrLinkedList<ThreadEntry>,
    running_thread: Option<*mut ThreadEntry>,
}

impl RunQueue {
    pub const fn new() -> Self {
        Self {
            lock: SpinLockFlag::new(),
            run_list: PtrLinkedList::new(),
            running_thread: None,
        }
    }

    pub fn init(&mut self) {
        /* Currently, do nothing */
    }

    pub fn start(&mut self) -> ! {
        let _lock = self.lock.lock();
        let thread = unsafe {
            self.run_list
                .get_first_entry_mut()
                .expect("There is no thread to start.")
        };

        thread.set_task_status(TaskStatus::Running);
        self.running_thread = Some(thread);
        drop(_lock);
        unsafe {
            get_kernel_manager_cluster()
                .task_manager
                .get_context_manager()
                .jump_to_context(thread.get_context())
        };
        panic!("Switching to the kernel process was failed.");
    }

    /// Sleep running thread and switch to next thread.
    ///
    /// This function will remove `thread` from run_queue_manager.
    /// This function assumes [Self::lock] must be lockable.
    /// This function will not change thread.task_status.
    ///
    /// `thread` must be locked and **`thread` must not be running thread**.
    #[allow(dead_code)]
    pub(super) fn remove_thread(&mut self, thread: &mut ThreadEntry) -> Result<(), TaskError> {
        assert!(thread.lock.is_locked());
        let interrupt_flag = InterruptManager::save_and_disable_local_irq();
        let _lock = self.lock.lock();
        let result = try {
            if thread.get_task_status() == TaskStatus::Running {
                Err(TaskError::InvalidThreadEntry)?;
            }
            thread.run_list.remove_from_list(&mut self.run_list);
        };
        drop(_lock);
        InterruptManager::restore_local_irq(interrupt_flag);
        return result;
    }

    /// Set current thread's status to Sleeping and call [Self::schedule].
    ///
    /// This function changes [Self::running_thread] to Sleep and call [Self::schedule].
    /// This does not check `ThreadEntry::sleep_list`.
    ///
    /// [Self::running_thread] must be unlocked.
    pub fn sleep_current_thread(
        &mut self,
        interrupt_flag: Option<StoredIrqData>,
    ) -> Result<(), TaskError> {
        assert!(!unsafe { &mut *self.running_thread.unwrap() }
            .lock
            .is_locked());
        let interrupt_flag =
            interrupt_flag.unwrap_or_else(|| InterruptManager::save_and_disable_local_irq());
        let _lock = self.lock.lock();
        let running_thread = unsafe { &mut *self.running_thread.unwrap() };
        let _running_thread_lock = running_thread.lock.lock();
        running_thread.set_task_status(TaskStatus::Sleeping);
        drop(_running_thread_lock);
        drop(_lock);
        self.schedule(Some(interrupt_flag), None);
        return Ok(());
    }

    /// Get current thread
    ///
    /// This function returns mut reference of current thread.
    ///
    /// To avoid dead lock of current thread's lock, the interrupt must be disabled.
    pub fn get_running_thread(&mut self) -> &mut ThreadEntry {
        assert!(!is_interrupt_enabled());
        unsafe { &mut *self.running_thread.unwrap() }
    }

    pub fn copy_running_thread_data(&self) -> Result<ThreadEntry, TaskError> {
        let interrupt_flag = InterruptManager::save_and_disable_local_irq();
        let _lock = self.lock.lock();
        let result = try {
            let running_thread = unsafe { &mut *self.running_thread.unwrap() };
            let _running_thread_lock = running_thread
                .lock
                .try_lock()
                .or(Err(TaskError::ThreadLockError))?;
            running_thread.copy_data()
        };
        drop(_lock);
        InterruptManager::restore_local_irq(interrupt_flag);
        return result;
    }

    /// Add thread into this run queue.
    ///
    /// `thread` must be locked.
    pub fn add_thread(&mut self, thread: &mut ThreadEntry) -> Result<(), TaskError> {
        assert!(thread.lock.is_locked());
        let interrupt_flag = InterruptManager::save_and_disable_local_irq();
        let _lock = self.lock.lock();
        let result = try {
            if let Some(last_thread) = unsafe { self.run_list.get_last_entry_mut() } {
                let _last_thread = last_thread
                    .lock
                    .try_lock()
                    .or(Err(TaskError::ThreadLockError))?;
                last_thread.run_list.insert_after(&mut thread.run_list);
            } else {
                thread.run_list.unset_prev_and_next();
                self.run_list
                    .set_first_entry(Some(&mut thread.run_list as *mut _));
            }
            thread.set_task_status(TaskStatus::CanRun);
        };
        drop(_lock);
        InterruptManager::restore_local_irq(interrupt_flag);
        return result;
    }

    /// This function checks current running thread and if it has to change task, this will call switch_to_next_thread.
    /// This function can be called in the interruptable status.([Self::lock] must be unlocked.)
    pub fn schedule(
        &mut self,
        interrupt_flag: Option<StoredIrqData>,
        current_context: Option<&ContextData>,
    ) {
        let interrupt_flag =
            interrupt_flag.unwrap_or_else(|| InterruptManager::save_and_disable_local_irq());
        let _lock = self.lock.lock();
        let running_thread = unsafe { &mut *self.running_thread.unwrap() };
        let _running_thread_lock = running_thread.lock.lock();
        let next_thread = if running_thread.get_task_status() == TaskStatus::Sleeping {
            if let Some(next_thread) = unsafe { running_thread.run_list.get_next_mut() } {
                let _next_thread_lock = next_thread.lock.lock();
                if let Some(prev_thread) = unsafe { running_thread.run_list.get_prev_mut() } {
                    let _prev_lock = prev_thread.lock.lock();
                    running_thread.run_list.remove_from_list(&mut self.run_list);
                } else {
                    next_thread.run_list.terminate_prev_entry();
                    if running_thread.run_list.get_prev_as_ptr().is_none() {
                        self.run_list
                            .set_first_entry(Some(&mut next_thread.run_list as *mut _));
                    }
                }
                next_thread
            } else {
                unsafe { self.run_list.get_first_entry_mut() }.unwrap()
            }
        } else {
            running_thread.set_task_status(TaskStatus::CanRun);
            if let Some(next_thread) = unsafe { running_thread.run_list.get_next_mut() } {
                next_thread
            } else {
                unsafe { self.run_list.get_first_entry_mut() }.unwrap()
            }
        };
        let running_thread_t_id = running_thread.get_t_id();
        let running_thread_p_id = running_thread.get_process().get_pid();
        drop(_running_thread_lock);

        next_thread.set_task_status(TaskStatus::Running);

        if running_thread_t_id == next_thread.get_t_id()
            && running_thread_p_id == next_thread.get_process().get_pid()
        {
            /* Same Task */
            drop(_lock);
            InterruptManager::restore_local_irq(interrupt_flag);
            return;
        }
        if let Some(c) = current_context {
            let _running_thread_lock = running_thread.lock.lock();
            running_thread.set_context(c);
            drop(_running_thread_lock);
            drop(_lock);
            InterruptManager::restore_local_irq(interrupt_flag); /* not good */
            unsafe {
                get_kernel_manager_cluster()
                    .task_manager
                    .get_context_manager()
                    .jump_to_context(next_thread.get_context());
            }
        } else {
            drop(_lock);
            InterruptManager::restore_local_irq(interrupt_flag); /* not good */
            unsafe {
                get_kernel_manager_cluster()
                    .task_manager
                    .get_context_manager()
                    .switch_context(running_thread.get_context(), next_thread.get_context());
            }
        }
    }
}
