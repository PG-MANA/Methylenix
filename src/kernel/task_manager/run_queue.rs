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
use crate::kernel::memory_manager::data_type::{Address, MSize};
use crate::kernel::memory_manager::object_allocator::ObjectAllocator;
use crate::kernel::memory_manager::pool_allocator::PoolAllocator;
use crate::kernel::memory_manager::MemoryManager;
use crate::kernel::ptr_linked_list::{PtrLinkedList, PtrLinkedListNode};
use crate::kernel::sync::spin_lock::{Mutex, SpinLockFlag, SpinLockFlagHolder};

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
    run_list_allocator: PoolAllocator<RunList>,
    should_recheck_priority: bool,
    should_reschedule: bool,
}

impl RunQueue {
    const DEFAULT_RUN_LIST_ALLOC_SIZE: usize = 12;
    pub const fn new() -> Self {
        Self {
            lock: SpinLockFlag::new(),
            run_list: PtrLinkedList::new(),
            running_thread: None,
            run_list_allocator: PoolAllocator::new(),
            should_recheck_priority: false,
            should_reschedule: false,
        }
    }

    pub fn init(
        &mut self,
        object_allocator: &mut ObjectAllocator,
        memory_manager: &Mutex<MemoryManager>,
    ) {
        let size = Self::DEFAULT_RUN_LIST_ALLOC_SIZE * core::mem::size_of::<RunList>();
        let pool_address = object_allocator
            .alloc(MSize::new(size), memory_manager)
            .expect("Cannot alloc pool for RunList");
        unsafe {
            self.run_list_allocator
                .set_initial_pool(pool_address.to_usize(), size);
        }
    }

    pub fn start(&mut self) -> ! {
        let _lock = self.lock.lock();
        let thread = Self::get_highest_priority_thread(&mut self.run_list)
            .expect("There is no thread to start.");

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

    fn get_highest_priority_thread(
        run_list: &mut PtrLinkedList<RunList>,
    ) -> Option<&mut ThreadEntry> {
        for list in run_list.iter_mut() {
            let list = unsafe { &mut *list };
            if let Some(t) = unsafe { list.thread_list.get_first_entry_mut() } {
                return Some(t);
            }
        }
        return None;
    }

    fn alloc_run_list(
        allocator: &mut PoolAllocator<RunList>,
        priority_level: u8,
    ) -> &'static mut RunList {
        let run_list = allocator
            .alloc()
            .expect("Cannot alloc RunList(TODO: alloc from object allocator)");
        *run_list = RunList::new(priority_level);
        let pointer = run_list as *mut _;
        run_list.chain.set_ptr(pointer);
        run_list
    }

    fn remove_target_thread(&mut self, thread: &mut ThreadEntry) -> Result<(), TaskError> {
        assert!(self.lock.is_locked());
        assert!(thread.lock.is_locked());
        let priority = thread.get_priority_level();
        for list in self.run_list.iter_mut() {
            let list = unsafe { &mut *list };
            if list.priority_level == priority {
                thread.run_list.remove_from_list(&mut list.thread_list);
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
        let interrupt_flag = InterruptManager::save_and_disable_local_irq();
        let _lock = self.lock.lock();
        let result = try {
            if thread.get_task_status() == TaskStatus::Running {
                Err(TaskError::InvalidThreadEntry)?;
            }
            self.remove_target_thread(thread)?;
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
    ///
    /// **Ensure that SpinLocks are unlocked before calling this function.**  
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
    /// **Be careful that other threads in this run queue must be unlocked.**
    pub fn add_thread(&mut self, thread: &mut ThreadEntry) -> Result<(), TaskError> {
        assert!(thread.lock.is_locked());
        let interrupt_flag = InterruptManager::save_and_disable_local_irq();
        let _lock = self.lock.lock();
        let result = try {
            let priority = thread.get_priority_level();
            if let Some(mut list) = unsafe { self.run_list.get_first_entry_mut() } {
                loop {
                    if list.priority_level == priority {
                        if let Some(last_entry) = unsafe { list.thread_list.get_last_entry_mut() } {
                            let _last_thread_lock = last_entry
                                .lock
                                .try_lock()
                                .or(Err(TaskError::ThreadLockError))?;
                            last_entry.run_list.insert_after(&mut thread.run_list);
                        } else {
                            list.thread_list.set_first_entry(Some(&mut thread.run_list));
                        }
                        break;
                    }
                    if list.priority_level > priority {
                        let run_list = Self::alloc_run_list(&mut self.run_list_allocator, priority);
                        run_list
                            .thread_list
                            .set_first_entry(Some(&mut thread.run_list));
                        if list.chain.get_prev_as_ptr().is_none() {
                            list.chain.insert_before(&mut run_list.chain);
                            self.run_list.set_first_entry(Some(&mut run_list.chain));
                        } else {
                            list.chain.insert_before(&mut run_list.chain);
                        }
                        break;
                    }
                    if let Some(next) = unsafe { list.chain.get_next_mut() } {
                        list = next;
                    } else {
                        let run_list = Self::alloc_run_list(&mut self.run_list_allocator, priority);
                        run_list
                            .thread_list
                            .set_first_entry(Some(&mut thread.run_list));
                        list.chain.insert_after(&mut run_list.chain);
                        break;
                    }
                }
            } else {
                let run_list = Self::alloc_run_list(&mut self.run_list_allocator, priority);
                run_list
                    .thread_list
                    .set_first_entry(Some(&mut thread.run_list));
                self.run_list.set_first_entry(Some(&mut run_list.chain));
            }
            thread.set_task_status(TaskStatus::CanRun);
            if self
                .running_thread
                .and_then(|r| {
                    Some(thread.get_priority_level() > unsafe { &*r }.get_priority_level())
                })
                .unwrap_or(false)
            {
                self.should_recheck_priority = true;
            }
            thread.time_slice = 5; /*Temporary*/
        };
        drop(_lock);
        InterruptManager::restore_local_irq(interrupt_flag);
        return result;
    }

    pub fn tick(&mut self) {
        let interrupt_flag = InterruptManager::save_and_disable_local_irq();
        let _lock = self.lock.lock();
        let running_thread = self.get_running_thread();
        running_thread.time_slice -= 1;
        if running_thread.time_slice < 1 {
            running_thread.time_slice = 5; /*Temporary*/
            self.should_reschedule = true;
        }
        self.get_running_thread().time_slice -= 1;
        drop(_lock);
        InterruptManager::restore_local_irq(interrupt_flag);
    }

    pub fn should_call_schedule(&self) -> bool {
        self.should_reschedule
    }

    /// This function checks current running thread and if it has to change task, this will call switch_to_next_thread.
    /// This function can be called in the interruptable status.([Self::lock] must be unlocked.)
    pub fn schedule(
        &mut self,
        interrupt_flag: Option<StoredIrqData>,
        current_context: Option<&ContextData>,
    ) {
        let get_prev_thread_lock =
            |running_thread: &mut ThreadEntry| -> Option<SpinLockFlagHolder> {
                if let Some(prev_thread) = unsafe { running_thread.run_list.get_prev_mut() } {
                    Some(prev_thread.lock.lock())
                } else {
                    None
                }
            };
        let interrupt_flag =
            interrupt_flag.unwrap_or_else(|| InterruptManager::save_and_disable_local_irq());
        let _lock = self.lock.lock();
        let running_thread = unsafe { &mut *self.running_thread.unwrap() };
        let _running_thread_lock = running_thread.lock.lock();
        let next_thread = if running_thread.get_task_status() == TaskStatus::Sleeping {
            if let Some(next_thread) = unsafe { running_thread.run_list.get_next_mut() } {
                let _next_thread_lock = next_thread.lock.lock();
                let _prev_lock = get_prev_thread_lock(running_thread);
                self.remove_target_thread(running_thread)
                    .expect("Cannot remove running thread from RunList");
                running_thread.run_list.unset_prev_and_next();
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
                running_thread.run_list.unset_prev_and_next();
                Self::get_highest_priority_thread(&mut self.run_list)
                    .expect("Cannot get thread to run")
            }
        } else {
            running_thread.set_task_status(TaskStatus::CanRun);
            if self.should_recheck_priority {
                self.should_recheck_priority = false;
                Self::get_highest_priority_thread(&mut self.run_list)
                    .expect("Cannot get thread to run")
            } else if let Some(next_thread) = unsafe { running_thread.run_list.get_next_mut() } {
                next_thread
            } else {
                Self::get_highest_priority_thread(&mut self.run_list)
                    .expect("Cannot get thread to run")
            }
        };

        let running_thread_t_id = running_thread.get_t_id();
        let running_thread_p_id = running_thread.get_process().get_pid();
        drop(_running_thread_lock);

        assert_eq!(next_thread.get_task_status(), TaskStatus::CanRun);

        next_thread.set_task_status(TaskStatus::Running);

        if running_thread_t_id == next_thread.get_t_id()
            && running_thread_p_id == next_thread.get_process().get_pid()
        {
            /* Same Task */
            drop(_lock);
            InterruptManager::restore_local_irq(interrupt_flag);
            return;
        }

        self.running_thread = Some(next_thread);
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
            return;
        }
    }
}
