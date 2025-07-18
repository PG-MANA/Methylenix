//!
//! Task Run Queue
//!
//! This module manages per-cpu run queue.
//! RunQueue will usually be used only specific cpu, but some methods may be called from other cpu,
//! therefore, it has SpinLock.

use super::{ProcessEntry, TaskError, TaskStatus, ThreadEntry};

use crate::arch::target_arch::context::context_data::ContextData;
use crate::arch::target_arch::device::cpu::is_interrupt_enabled;
use crate::arch::target_arch::interrupt::{InterruptManager, StoredIrqData};

use crate::kernel::{
    collections::ptr_linked_list::{PtrLinkedList, PtrLinkedListNode},
    manager_cluster::get_kernel_manager_cluster,
    memory_manager::{MemoryError, slab_allocator::LocalSlabAllocator},
    sync::spin_lock::{SpinLockFlag, SpinLockFlagHolder},
    timer_manager::GlobalTimerManager,
};

use core::mem::offset_of;

struct RunList {
    list: PtrLinkedListNode<Self>,
    priority_level: u8,
    thread_list: PtrLinkedList<ThreadEntry>,
}

impl RunList {
    const fn new(priority_level: u8) -> Self {
        Self {
            list: PtrLinkedListNode::new(),
            priority_level,
            thread_list: PtrLinkedList::new(),
        }
    }
}

pub struct RunQueue {
    lock: SpinLockFlag,
    run_list: PtrLinkedList<RunList>,
    expired_list: PtrLinkedList<RunList>,
    idle_thread: *mut ThreadEntry,
    running_thread: Option<*mut ThreadEntry>,
    run_list_allocator: LocalSlabAllocator<RunList>,
    should_recheck_priority: bool,
    should_reschedule: bool,
    number_of_threads: usize,
}

macro_rules! get_highest_priority_thread {
    ($l:expr) => {{
        use core::mem::offset_of;
        let mut e = None;
        for list in unsafe { $l.iter_mut(offset_of!(RunList, list)) } {
            if let Some(t) = list
                .thread_list
                .get_first_entry_mut(offset_of!(ThreadEntry, run_list))
                .map(|t| unsafe { &mut *t })
            {
                e = Some(t);
                break;
            }
        }
        e
    }};
}

impl RunQueue {
    pub const fn new() -> Self {
        Self {
            lock: SpinLockFlag::new(),
            run_list: PtrLinkedList::new(),
            expired_list: PtrLinkedList::new(),
            idle_thread: core::ptr::null_mut(),
            running_thread: None,
            run_list_allocator: LocalSlabAllocator::new(),
            should_recheck_priority: false,
            should_reschedule: false,
            number_of_threads: 0,
        }
    }

    pub fn init(&mut self) -> Result<(), MemoryError> {
        self.run_list_allocator.init()
    }

    pub fn set_idle_thread(&mut self, idle_thread: &mut ThreadEntry) {
        self.idle_thread = idle_thread;
        idle_thread.run_list = PtrLinkedListNode::new();
        idle_thread.set_task_status(TaskStatus::Running);
        self.number_of_threads += 1;
    }

    pub fn start(&mut self) -> ! {
        let _irq = InterruptManager::save_and_disable_local_irq();
        let _lock = self.lock.lock();
        let thread = get_highest_priority_thread!(self.run_list)
            .unwrap_or(unsafe { &mut *self.idle_thread });

        thread.set_task_status(TaskStatus::Running);
        self.running_thread = Some(thread);
        drop(_lock);
        unsafe {
            get_kernel_manager_cluster()
                .task_manager
                .get_context_manager()
                .jump_to_context(thread.get_context(), true)
        };
        panic!("Switching to the kernel process was failed.");
    }

    /// Get the number of running threads.
    ///
    /// This function returns the number of running threads in this run queue.
    /// This does not lock `Self::lock`.
    pub fn get_number_of_running_threads(&self) -> usize {
        let irq = InterruptManager::save_and_disable_local_irq();
        let _lock = self.lock.lock();
        let result = self.number_of_threads;
        drop(_lock);
        InterruptManager::restore_local_irq(irq);
        result
    }

    fn remove_target_thread(&mut self, thread: &mut ThreadEntry) -> Result<(), TaskError> {
        assert!(self.lock.is_locked());
        assert!(thread.lock.is_locked());
        let priority = thread.get_priority_level();
        for list in unsafe { self.run_list.iter_mut(offset_of!(RunList, list)) } {
            if list.priority_level == priority {
                unsafe { list.thread_list.remove(&mut thread.run_list) };
                return Ok(());
            }
        }
        Err(TaskError::InvalidThreadEntry)
    }

    /// Sleep running thread and switch to the next thread.
    ///
    /// This function will remove `thread` from run_queue_manager.
    /// This function assumes [Self::lock] must be lockable.
    /// This function will not change thread.task_status.
    ///
    /// `thread` must be locked and **`thread` must not be running thread**.
    #[allow(dead_code)]
    pub(super) fn remove_thread(&mut self, thread: &mut ThreadEntry) -> Result<(), TaskError> {
        assert!(thread.lock.is_locked());
        assert!(!self.lock.is_locked());
        let irq = InterruptManager::save_and_disable_local_irq();
        let _lock = self.lock.lock();
        let result = try {
            if thread.get_task_status() == TaskStatus::Running {
                Err(TaskError::InvalidThreadEntry)?;
            }
            self.remove_target_thread(thread)?;
            self.number_of_threads -= 1;
        };
        drop(_lock);
        InterruptManager::restore_local_irq(irq);
        result
    }

    /// Set the current thread's status to Sleeping and call [Self::schedule].
    ///
    /// This function changes [Self::running_thread] to Sleep and call [Self::schedule].
    /// `task_status` is set into the current thread (it must not be Running).
    /// This does not check `ThreadEntry::sleep_list`.
    ///
    /// [Self::running_thread] must be unlocked.
    ///
    /// **Ensure that SpinLocks are unlocked before calling this function.**
    pub fn sleep_current_thread(
        &mut self,
        interrupt_flag: Option<StoredIrqData>,
        task_status: TaskStatus,
    ) -> Result<(), TaskError> {
        let irq = interrupt_flag.unwrap_or_else(InterruptManager::save_and_disable_local_irq);
        let lock = self.lock.lock();
        let running_thread = unsafe { &mut *self.running_thread.unwrap() };
        let _running_thread_lock = running_thread.lock.lock();
        running_thread.set_task_status(task_status);
        self._schedule(None, Some(irq), Some(lock), Some(_running_thread_lock));
        Ok(())
    }

    /// Get current thread
    ///
    /// This function returns mut reference of current thread.
    ///
    /// To avoid deadlock of current thread's lock, the interrupt must be disabled.
    pub fn get_running_thread(&mut self) -> &mut ThreadEntry {
        assert!(!is_interrupt_enabled());
        unsafe { &mut *self.running_thread.unwrap() }
    }

    pub fn get_running_process(&mut self) -> &mut ProcessEntry {
        unsafe { &mut *(&mut *self.running_thread.unwrap()).get_process_mut() }
    }

    pub fn get_running_pid(&self) -> usize {
        if let Some(t) = self.running_thread {
            unsafe { &*t }.get_process().get_pid()
        } else {
            super::KERNEL_PID
        }
    }

    pub fn copy_running_thread_data(&self) -> Result<ThreadEntry, TaskError> {
        let irq = InterruptManager::save_and_disable_local_irq();
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
        InterruptManager::restore_local_irq(irq);
        result
    }

    fn _add_thread(
        &mut self,
        thread: &mut ThreadEntry,
        is_expired_list: bool,
    ) -> Result<(), TaskError> {
        macro_rules! alloc_run_list {
            ($a:expr, $p:expr) => {
                $a.alloc().map(|mut r| {
                    let r = unsafe { &mut *r.as_mut() };
                    crate::kernel::collections::init_struct!(*r, RunList::new($p));
                    r
                })
            };
        }

        assert!(self.lock.is_locked());
        let priority = thread.get_priority_level();
        let target_list = if is_expired_list {
            &mut self.expired_list
        } else {
            &mut self.run_list
        };
        if let Some(mut list) = target_list
            .get_first_entry_mut(offset_of!(RunList, list))
            .map(|t| unsafe { &mut *t })
        {
            loop {
                if list.priority_level == priority {
                    if let Some(last_entry) = list
                        .thread_list
                        .get_last_entry_mut(offset_of!(ThreadEntry, run_list))
                        .map(|t| unsafe { &mut *t })
                    {
                        let _last_thread_lock = last_entry
                            .lock
                            .try_lock()
                            .or(Err(TaskError::ThreadLockError))?;
                        unsafe { list.thread_list.insert_tail(&mut thread.run_list) };
                    } else {
                        unsafe { list.thread_list.insert_head(&mut thread.run_list) };
                    }
                    break;
                }
                if list.priority_level > priority {
                    let run_list = alloc_run_list!(self.run_list_allocator, priority)?;
                    unsafe {
                        run_list.thread_list.insert_head(&mut thread.run_list);
                        target_list.insert_before(&mut list.list, &mut run_list.list);
                    }
                    break;
                }
                if let Some(next) = list
                    .list
                    .get_next_mut(offset_of!(RunList, list))
                    .map(|t| unsafe { &mut *t })
                {
                    list = next;
                } else {
                    let run_list = alloc_run_list!(self.run_list_allocator, priority)?;
                    unsafe {
                        run_list.thread_list.insert_head(&mut thread.run_list);
                        target_list.insert_tail(&mut run_list.list);
                    }
                    break;
                }
            }
        } else {
            let run_list = alloc_run_list!(self.run_list_allocator, priority)?;
            unsafe {
                run_list.thread_list.insert_head(&mut thread.run_list);
                target_list.insert_head(&mut run_list.list);
            }
        }

        if !is_expired_list {
            thread.set_task_status(TaskStatus::Running);
            if self
                .running_thread
                .map(|r| thread.get_priority_level() > unsafe { &*r }.get_priority_level())
                .unwrap_or(false)
            {
                self.should_recheck_priority = true;
                self.should_reschedule = true;
            }
            self.number_of_threads += 1;
            thread.set_time_slice(
                self.number_of_threads,
                GlobalTimerManager::TIMER_INTERVAL_MS,
            );
        }
        Ok(())
    }

    /// Add thread into this run queue.
    ///
    /// `thread` must be locked.
    ///
    /// **Be careful that other threads in this run queue must be unlocked.**
    pub fn add_thread(&mut self, thread: &mut ThreadEntry) -> Result<(), TaskError> {
        assert!(thread.lock.is_locked());
        let irq = InterruptManager::save_and_disable_local_irq();
        let _lock = self.lock.lock();
        let result = self._add_thread(thread, false);
        drop(_lock);
        InterruptManager::restore_local_irq(irq);
        result
    }

    /// Add thread into this run queue from **other cpus**.
    ///
    /// This function will add the thread into this run queue.
    /// The return value indicates if you should notify the cpu having this run queue to reschedule.
    ///
    /// `thread` must be locked.
    pub fn assign_thread(&mut self, thread: &mut ThreadEntry) -> Result<bool, TaskError> {
        assert!(thread.lock.is_locked());
        let irq = InterruptManager::save_and_disable_local_irq();
        let _lock = self.lock.lock(); /* To avoid task switch holding other CPU's run_queue_lock */
        self._add_thread(thread, false)?;
        let thread_priority = thread.get_priority_level();
        let should_reschedule =
            if let Some(running_thread) = self.running_thread.map(|r| unsafe { &*r }) {
                let _running_thread_lock = running_thread.lock.lock();
                let running_thread_priority = running_thread.get_priority_level();
                if thread_priority < running_thread_priority {
                    self.should_reschedule = true;
                    true
                } else {
                    false
                }
            } else {
                false
            };
        drop(_lock);
        InterruptManager::restore_local_irq(irq);
        Ok(should_reschedule)
    }

    pub fn tick(&mut self) {
        let interrupt_flag = InterruptManager::save_and_disable_local_irq();
        let _lock = self.lock.lock();
        let running_thread = self.get_running_thread();
        if running_thread.time_slice < 1 {
            running_thread.time_slice = 0;
            self.should_reschedule = true;
        } else {
            running_thread.time_slice -= 1;
        }
        drop(_lock);
        InterruptManager::restore_local_irq(interrupt_flag);
    }

    pub fn should_call_schedule(&self) -> bool {
        self.should_reschedule
    }

    fn set_thread_to_expired_thread(&mut self, thread: &mut ThreadEntry) -> Result<(), TaskError> {
        assert!(self.lock.is_locked());
        self._add_thread(thread, true)
    }

    fn _schedule(
        &mut self,
        current_context: Option<&ContextData>,
        interrupt_flag: Option<StoredIrqData>,
        lock: Option<SpinLockFlagHolder>,
        running_thread_lock: Option<SpinLockFlagHolder>,
    ) {
        let interrupt_flag =
            interrupt_flag.unwrap_or_else(InterruptManager::save_and_disable_local_irq);
        let _lock = lock.unwrap_or_else(|| self.lock.lock());
        let get_prev_thread_lock =
            |running_thread: &mut ThreadEntry| -> Option<SpinLockFlagHolder> {
                if let Some(prev_thread) = running_thread
                    .run_list
                    .get_prev_mut(offset_of!(ThreadEntry, run_list))
                    .map(|t| unsafe { &mut *t })
                {
                    Some(prev_thread.lock.lock())
                } else {
                    None
                }
            };
        let running_thread = unsafe { &mut *self.running_thread.unwrap() };
        let _running_thread_lock =
            running_thread_lock.unwrap_or_else(|| running_thread.lock.lock());

        macro_rules! get_next_thread {
            () => {
                if let Some(t) = get_highest_priority_thread!(&mut self.run_list) {
                    t
                } else {
                    core::mem::swap(&mut self.run_list, &mut self.expired_list);
                    if let Some(t) = get_highest_priority_thread!(&mut self.run_list) {
                        t
                    } else {
                        unsafe { &mut *self.idle_thread }
                    }
                }
            };
        }
        let next_thread = if let Some(next_thread) = running_thread
            .run_list
            .get_next_mut(offset_of!(ThreadEntry, run_list))
            .map(|t| unsafe { &mut *t })
        {
            assert!(!core::ptr::eq(running_thread, self.idle_thread));

            let _next_thread_lock = next_thread.lock.lock();
            let _prev_lock = get_prev_thread_lock(running_thread);
            self.remove_target_thread(running_thread)
                .expect("Cannot remove running thread from RunList");

            if running_thread.get_task_status() == TaskStatus::Running {
                running_thread.set_time_slice(
                    self.number_of_threads,
                    GlobalTimerManager::TIMER_INTERVAL_MS,
                );
                self.set_thread_to_expired_thread(running_thread)
                    .expect("Failed to add running thread to expired list");
            }

            if self.should_recheck_priority {
                self.should_recheck_priority = false;
                get_highest_priority_thread!(self.run_list).unwrap_or(next_thread)
            } else {
                next_thread
            }
        } else {
            if running_thread as *mut _ == self.idle_thread {
                running_thread.set_time_slice(
                    self.number_of_threads,
                    GlobalTimerManager::TIMER_INTERVAL_MS,
                );
            } else {
                let _prev_lock = get_prev_thread_lock(running_thread);
                self.remove_target_thread(running_thread)
                    .expect("Cannot remove running thread from RunList");

                if running_thread.get_task_status() == TaskStatus::Running {
                    running_thread.set_time_slice(
                        self.number_of_threads,
                        GlobalTimerManager::TIMER_INTERVAL_MS,
                    );
                    self.set_thread_to_expired_thread(running_thread)
                        .expect("Failed to add running thread to expired list");
                }
            }
            get_next_thread!()
        };

        let running_thread_p_id = running_thread.get_process().get_pid();
        let next_thread_p_id = next_thread.get_process().get_pid();

        assert_eq!(next_thread.get_task_status(), TaskStatus::Running);

        if running_thread.get_t_id() == next_thread.get_t_id()
            && running_thread_p_id == next_thread_p_id
        {
            /* Same Task */
            drop(_running_thread_lock);
            drop(_lock);
            InterruptManager::restore_local_irq(interrupt_flag);
            return;
        }

        let mut should_use_switch_context = true;
        if let Some(c) = current_context {
            running_thread.set_context(c);
            should_use_switch_context = false;
        }
        drop(_running_thread_lock);

        self.should_reschedule = false;
        self.running_thread = Some(next_thread);

        if running_thread_p_id != next_thread_p_id {
            let memory_manager = next_thread.get_process().get_memory_manager();
            if !memory_manager.is_null() && next_thread_p_id != 0 {
                let memory_manager = unsafe { &mut *memory_manager };
                memory_manager
                    .clone_kernel_memory_pages_if_needed()
                    .expect("Failed to copy the page table of kernel area");
                memory_manager.set_paging_table();
            }
        }
        if !should_use_switch_context {
            drop(_lock);
            unsafe {
                get_kernel_manager_cluster()
                    .task_manager
                    .get_context_manager()
                    .jump_to_context(next_thread.get_context(), true);
            }
        } else {
            drop(_lock);
            unsafe {
                get_kernel_manager_cluster()
                    .task_manager
                    .get_context_manager()
                    .switch_context(
                        running_thread.get_context(),
                        next_thread.get_context(),
                        true,
                    );
            }
        }
    }

    /// This function checks current running thread and if it has to change task, this will call switch_to_next_thread.
    /// This function can be called in the interruptable status.([Self::lock] must be unlocked.)
    pub fn schedule(&mut self, current_context: Option<&ContextData>) {
        self._schedule(current_context, None, None, None)
    }
}
