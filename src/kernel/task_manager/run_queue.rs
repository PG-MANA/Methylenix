//!
//! Task Run Queue
//!
//! This module manages per-cpu run queue.
//! RunQueue will be usually used only specific cpu, but some methods may be called from other cpu,
//! therefore, it has SpinLock.

use super::thread_entry::ThreadEntry;
use super::{TaskError, TaskStatus};

use crate::arch::target_arch::context::context_data::ContextData;
use crate::arch::target_arch::device::cpu::is_interrupt_enabled;
use crate::arch::target_arch::interrupt::{InterruptManager, StoredIrqData};

use crate::kernel::collections::ptr_linked_list::{PtrLinkedList, PtrLinkedListNode};
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::slab_allocator::LocalSlabAllocator;
use crate::kernel::memory_manager::MemoryError;
use crate::kernel::sync::spin_lock::{SpinLockFlag, SpinLockFlagHolder};
use crate::kernel::timer_manager::GlobalTimerManager;

struct RunList {
    priority_level: u8,
    thread_list: PtrLinkedList<ThreadEntry>,
    chain: PtrLinkedListNode<Self>,
}

impl RunList {
    const fn new(priority_level: u8) -> Self {
        Self {
            priority_level,
            thread_list: PtrLinkedList::new(),
            chain: PtrLinkedListNode::new(),
        }
    }
}

pub struct RunQueue {
    lock: SpinLockFlag,
    run_list: PtrLinkedList<RunList>,
    running_thread: Option<*mut ThreadEntry>,
    run_list_allocator: LocalSlabAllocator<RunList>,
    should_recheck_priority: bool,
    should_reschedule: bool,
    number_of_threads: usize,
}

impl RunQueue {
    pub const fn new() -> Self {
        Self {
            lock: SpinLockFlag::new(),
            run_list: PtrLinkedList::new(),
            running_thread: None,
            run_list_allocator: LocalSlabAllocator::new(0),
            should_recheck_priority: false,
            should_reschedule: false,
            number_of_threads: 0,
        }
    }

    pub fn init(&mut self) -> Result<(), MemoryError> {
        self.run_list_allocator.init()
    }

    pub fn start(&mut self) -> ! {
        let irq = InterruptManager::save_and_disable_local_irq();
        let _lock = self.lock.lock();
        let thread = Self::get_highest_priority_thread(&mut self.run_list)
            .expect("There is no thread to start.");

        thread.set_task_status(TaskStatus::Running);
        self.running_thread = Some(thread);
        drop(_lock);
        InterruptManager::restore_local_irq(irq);
        unsafe {
            get_kernel_manager_cluster()
                .task_manager
                .get_context_manager()
                .jump_to_context(thread.get_context())
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
        return result;
    }

    fn get_highest_priority_thread(
        run_list: &mut PtrLinkedList<RunList>,
    ) -> Option<&mut ThreadEntry> {
        for list in unsafe { run_list.iter_mut(offset_of!(RunList, chain)) } {
            if let Some(t) = unsafe {
                list.thread_list
                    .get_first_entry_mut(offset_of!(ThreadEntry, run_list))
            } {
                return Some(t);
            }
        }
        return None;
    }

    fn alloc_run_list(
        allocator: &mut LocalSlabAllocator<RunList>,
        priority_level: u8,
    ) -> &'static mut RunList {
        let run_list = allocator.alloc().expect("Failed to alloc RunList");
        *run_list = RunList::new(priority_level);
        run_list
    }

    fn remove_target_thread(&mut self, thread: &mut ThreadEntry) -> Result<(), TaskError> {
        assert!(self.lock.is_locked());
        assert!(thread.lock.is_locked());
        let priority = thread.get_priority_level();
        for list in unsafe { self.run_list.iter_mut(offset_of!(RunList, chain)) } {
            if list.priority_level == priority {
                list.thread_list.remove(&mut thread.run_list);
                self.number_of_threads -= 1;
                return Ok(());
            }
        }
        return Err(TaskError::InvalidThreadEntry);
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
        assert!(!self.lock.is_locked());
        let irq = InterruptManager::save_and_disable_local_irq();
        let _lock = self.lock.lock();
        let result = try {
            if thread.get_task_status() == TaskStatus::Running {
                Err(TaskError::InvalidThreadEntry)?;
            }
            self.remove_target_thread(thread)?;
        };
        drop(_lock);
        InterruptManager::restore_local_irq(irq);
        return result;
    }

    /// Set current thread's status to Sleeping and call [Self::schedule].
    ///
    /// This function changes [Self::running_thread] to Sleep and call [Self::schedule].
    /// `task_status` is set into current thread(it must not be Running).
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
        assert!(!unsafe { &mut *self.running_thread.unwrap() }
            .lock
            .is_locked());
        assert_ne!(task_status, TaskStatus::Running);

        let irq = interrupt_flag.unwrap_or_else(|| InterruptManager::save_and_disable_local_irq());
        let lock = self.lock.lock();
        let running_thread = unsafe { &mut *self.running_thread.unwrap() };
        let _running_thread_lock = running_thread.lock.lock();
        running_thread.set_task_status(task_status);
        drop(_running_thread_lock);
        self._schedule(None, Some(irq), Some(lock));
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
        return result;
    }

    fn _add_thread(&mut self, thread: &mut ThreadEntry) -> Result<(), TaskError> {
        assert!(self.lock.is_locked());
        let priority = thread.get_priority_level();
        if let Some(mut list) = unsafe {
            self.run_list
                .get_first_entry_mut(offset_of!(RunList, chain))
        } {
            loop {
                if list.priority_level == priority {
                    if let Some(last_entry) = unsafe {
                        list.thread_list
                            .get_last_entry_mut(offset_of!(ThreadEntry, run_list))
                    } {
                        let _last_thread_lock = last_entry
                            .lock
                            .try_lock()
                            .or(Err(TaskError::ThreadLockError))?;
                        list.thread_list.insert_tail(&mut thread.run_list);
                    } else {
                        list.thread_list.insert_head(&mut thread.run_list);
                    }
                    break;
                }
                if list.priority_level > priority {
                    let run_list = Self::alloc_run_list(&mut self.run_list_allocator, priority);
                    run_list.thread_list.insert_head(&mut thread.run_list);
                    self.run_list
                        .insert_before(&mut list.chain, &mut run_list.chain);
                    break;
                }
                if let Some(next) = unsafe { list.chain.get_next_mut(offset_of!(RunList, chain)) } {
                    list = next;
                } else {
                    let run_list = Self::alloc_run_list(&mut self.run_list_allocator, priority);
                    run_list.thread_list.insert_head(&mut thread.run_list);
                    self.run_list.insert_tail(&mut run_list.chain);
                    break;
                }
            }
        } else {
            let run_list = Self::alloc_run_list(&mut self.run_list_allocator, priority);
            run_list.thread_list.insert_head(&mut thread.run_list);
            self.run_list.insert_head(&mut run_list.chain);
        }
        thread.set_task_status(TaskStatus::Running);
        if self
            .running_thread
            .and_then(|r| Some(thread.get_priority_level() > unsafe { &*r }.get_priority_level()))
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
        return Ok(());
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
        let result = self._add_thread(thread);
        drop(_lock);
        InterruptManager::restore_local_irq(irq);
        return result;
    }

    /// Add thread into this run queue from **other cpus**.
    ///
    /// This function will add the thread into this run queue.
    /// The return value indicates if should notify the cpu having this run queue to reschedule.
    ///
    /// `thread` must be locked.
    pub fn assign_thread(&mut self, thread: &mut ThreadEntry) -> Result<bool, TaskError> {
        assert!(thread.lock.is_locked());
        let _lock = self.lock.lock();
        /* To avoid task switch holding other cpu's run_queue_lock */
        let irq = InterruptManager::save_and_disable_local_irq();
        self._add_thread(thread)?;
        let thread_priority = thread.get_priority_level();
        let should_reschedule =
            if let Some(running_thread) = self.running_thread.and_then(|r| Some(unsafe { &*r })) {
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
        return Ok(should_reschedule);
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

    fn _schedule(
        &mut self,
        current_context: Option<&ContextData>,
        interrupt_flag: Option<StoredIrqData>,
        lock: Option<SpinLockFlagHolder>,
    ) {
        let interrupt_flag =
            interrupt_flag.unwrap_or_else(|| InterruptManager::save_and_disable_local_irq());
        let _lock = lock.unwrap_or_else(|| self.lock.lock());
        let get_prev_thread_lock =
            |running_thread: &mut ThreadEntry| -> Option<SpinLockFlagHolder> {
                if let Some(prev_thread) = unsafe {
                    running_thread
                        .run_list
                        .get_prev_mut(offset_of!(ThreadEntry, run_list))
                } {
                    Some(prev_thread.lock.lock())
                } else {
                    None
                }
            };
        let running_thread = unsafe { &mut *self.running_thread.unwrap() };
        let _running_thread_lock = running_thread.lock.lock();
        let next_thread = if running_thread.get_task_status() != TaskStatus::Running {
            if let Some(next_thread) = unsafe {
                running_thread
                    .run_list
                    .get_next_mut(offset_of!(ThreadEntry, run_list))
            } {
                let _next_thread_lock = next_thread.lock.lock();
                let _prev_lock = get_prev_thread_lock(running_thread);
                self.remove_target_thread(running_thread)
                    .expect("Cannot remove running thread from RunList");
                if self.should_recheck_priority {
                    self.should_recheck_priority = false;
                    Self::get_highest_priority_thread(&mut self.run_list).unwrap_or(next_thread)
                } else {
                    next_thread
                }
            } else {
                let _prev_lock = get_prev_thread_lock(running_thread);
                self.remove_target_thread(running_thread)
                    .expect("Cannot remove running thread from RunList");
                Self::get_highest_priority_thread(&mut self.run_list)
                    .expect("Cannot get thread to run")
            }
        } else {
            running_thread.set_time_slice(
                self.number_of_threads,
                GlobalTimerManager::TIMER_INTERVAL_MS,
            );
            if self.should_recheck_priority {
                self.should_recheck_priority = false;
                Self::get_highest_priority_thread(&mut self.run_list)
                    .expect("Cannot get thread to run")
            } else if let Some(next_thread) = unsafe {
                running_thread
                    .run_list
                    .get_next_mut(offset_of!(ThreadEntry, run_list))
            } {
                next_thread
            } else {
                Self::get_highest_priority_thread(&mut self.run_list)
                    .expect("Cannot get thread to run")
            }
        };

        let running_thread_p_id = running_thread.get_process().get_pid();

        assert_eq!(next_thread.get_task_status(), TaskStatus::Running);

        if running_thread.get_t_id() == next_thread.get_t_id()
            && running_thread_p_id == next_thread.get_process().get_pid()
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

        if running_thread_p_id != next_thread.get_process().get_pid() {
            let memory_manager = next_thread.get_process().get_memory_manager();
            if !memory_manager.is_null() {
                let memory_manager = unsafe { &mut *memory_manager };
                memory_manager
                    .clone_kernel_memory_pages_if_needed()
                    .expect("Failed to copy the page table of kernel area");
                memory_manager.set_paging_table();
            }
        }
        if !should_use_switch_context {
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

    /// This function checks current running thread and if it has to change task, this will call switch_to_next_thread.
    /// This function can be called in the interruptable status.([Self::lock] must be unlocked.)
    pub fn schedule(&mut self, current_context: Option<&ContextData>) {
        self._schedule(current_context, None, None)
    }
}
