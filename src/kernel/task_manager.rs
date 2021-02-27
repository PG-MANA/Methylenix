//!
//! Task Manager
//!
//! This manager is the frontend of task management system.
//! Task management system has two struct, arch-independent and depend on arch.

mod process_entry;
pub mod run_queue_manager;
mod thread_entry;
pub mod wait_queue;
pub mod work_queue;

use self::process_entry::ProcessEntry;
use self::thread_entry::ThreadEntry;

use crate::arch::target_arch::context::{context_data::ContextData, ContextManager};
use crate::arch::target_arch::interrupt::InterruptManager;

use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::data_type::MSize;
use crate::kernel::memory_manager::object_allocator::cache_allocator::CacheAllocator;
use crate::kernel::memory_manager::MemoryError;
use crate::kernel::ptr_linked_list::PtrLinkedList;
use crate::kernel::sync::spin_lock::SpinLockFlag;
use crate::kernel::task_manager::run_queue_manager::RunQueueManager;

pub struct TaskManager {
    lock: SpinLockFlag,
    kernel_process: *mut ProcessEntry,
    idle_thread: *mut ThreadEntry,
    context_manager: ContextManager,
    process_entry_pool: CacheAllocator<ProcessEntry>,
    thread_entry_pool: CacheAllocator<ThreadEntry>,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum TaskSignal {
    Normal,
    Stop,
    Kill,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum TaskStatus {
    New,
    Sleeping,
    CanRun,
    Running,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum ProcessStatus {
    New,
    Normal,
    Zombie,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum TaskError {
    MemoryError(MemoryError),
    ThreadLockError,
    InvalidProcessEntry,
    InvalidThreadEntry,
}

impl From<MemoryError> for TaskError {
    fn from(m: MemoryError) -> Self {
        Self::MemoryError(m)
    }
}

impl TaskManager {
    const NUM_OF_INITIAL_THREAD_ENTRIES: usize = 6;
    const NUM_OF_INITIAL_PROCESS_ENTRIES: usize = 6;

    pub const fn new() -> Self {
        Self {
            lock: SpinLockFlag::new(),
            kernel_process: unsafe { core::ptr::null_mut() },
            idle_thread: unsafe { core::ptr::null_mut() },
            context_manager: ContextManager::new(),
            process_entry_pool: CacheAllocator::new(ProcessEntry::PROCESS_ENTRY_ALIGN_ORDER),
            thread_entry_pool: CacheAllocator::new(ThreadEntry::THREAD_ENTRY_ALIGN_ORDER),
        }
    }

    /// Init TaskManager
    ///
    /// This function setups memory pools and create kernel process.
    /// The kernel process has two threads created with kernel_main_context and idle_context.
    /// After that, this function will set those threads into run_queue_manager.
    ///
    /// **Attention: MemoryManager must be unlocked.**
    pub fn init(
        &mut self,
        context_manager: ContextManager,
        kernel_main_context: ContextData,
        idle_context: ContextData,
        run_queue_manager: &mut RunQueueManager,
    ) {
        let _lock = self.lock.lock();
        let mut memory_manager = get_kernel_manager_cluster().memory_manager.lock().unwrap();
        self.context_manager = context_manager;

        self.process_entry_pool
            .init(Self::NUM_OF_INITIAL_PROCESS_ENTRIES, &mut memory_manager)
            .expect("Allocating the pool was failed:");
        self.thread_entry_pool
            .init(Self::NUM_OF_INITIAL_THREAD_ENTRIES, &mut memory_manager)
            .expect("Allocating the pool was failed: {:?}");

        let memory_manager = &get_kernel_manager_cluster().memory_manager;

        /* Create the kernel process and threads */
        let kernel_process = self.process_entry_pool.alloc(Some(memory_manager)).unwrap();
        let main_thread = self.thread_entry_pool.alloc(Some(memory_manager)).unwrap();
        let idle_thread = self.thread_entry_pool.alloc(Some(memory_manager)).unwrap();

        main_thread.init(kernel_process, 0, 0, kernel_main_context);
        idle_thread.init(kernel_process, 0, i8::MIN, idle_context);

        kernel_process.init(
            0,
            unsafe { core::ptr::null_mut() },
            &mut [main_thread, idle_thread],
            memory_manager as *const _,
            0,
        );

        let _process_lock = kernel_process.lock.lock();
        let _main_thread_lock = main_thread.lock.lock();
        let _idle_thread_lock = idle_thread.lock.lock();
        kernel_process.set_ptr_to_list();
        main_thread.set_ptr_to_list();
        idle_thread.set_ptr_to_list();

        /* Set threads to run_queue_manager */
        run_queue_manager.add_thread(main_thread);
        run_queue_manager.add_thread(idle_thread);

        self.kernel_process = kernel_process;
        self.idle_thread = idle_thread;
        return;
    }

    /// Init idle thread for additional processors.
    ///
    /// This function forks idle thread and sets it to run_queue_manager.
    pub fn init_idle(
        &mut self,
        idle_fn: fn() -> !,
        run_queue_manager: &mut RunQueueManager,
    ) -> Result<(), TaskError> {
        let idle_thread = unsafe { &mut *self.idle_thread };
        let mut forked_thread = self.fork_system_thread(
            idle_thread,
            idle_fn,
            Some(ContextManager::IDLE_THREAD_STACK_SIZE),
        )?;
        let _lock = forked_thread.lock.lock();
        run_queue_manager.add_thread(forked_thread);
        return Ok(());
    }

    /// Fork `thread` and create `ThreadEntry`.
    ///
    /// This function copies data from `thread` and alloc `ThreadEntry` from `Self::thread_entry_pool`.
    /// `thread` must be unlocked.
    fn fork_system_thread(
        &mut self,
        thread: &mut ThreadEntry,
        entry_address: fn() -> !,
        stack_size: Option<MSize>,
    ) -> Result<&'static mut ThreadEntry, TaskError> {
        /* self.lock must be locked. */
        let new_thread = self
            .thread_entry_pool
            .alloc(Some(&get_kernel_manager_cluster().memory_manager))?;
        let _original_thread_lock = thread.lock.lock();
        let new_context = self.context_manager.fork_system_context(
            thread.get_context(),
            entry_address,
            stack_size,
        )?;
        let parent_process = thread.get_process_mut();
        new_thread.init(
            parent_process as *mut _,
            thread.get_privilege_level(),
            thread.get_priority_level(),
            new_context,
        );
        drop(_original_thread_lock);
        let _process_lock = parent_process
            .lock
            .try_lock()
            .or(Err(TaskError::ThreadLockError))?;
        parent_process.add_thread(new_thread);
        return Ok(new_thread);
    }

    pub fn create_kernel_thread_for_init(
        &mut self,
        entry_address: fn() -> !,
        stack_size: Option<MSize>,
        priority_level: i8,
    ) -> Result<&'static mut ThreadEntry, TaskError> {
        let interrupt_flag = InterruptManager::save_and_disable_local_irq();
        let _lock = self.lock.lock();
        let result = try {
            let mut main_thread = unsafe { &mut *self.idle_thread };
            let forked_thread =
                self.fork_system_thread(&mut main_thread, entry_address, stack_size)?;
            let _forked_thread_lock = forked_thread.lock.lock();
            forked_thread.set_priority_level(priority_level);
            forked_thread.set_task_status(TaskStatus::New);
            forked_thread
        };

        drop(_lock);
        InterruptManager::restore_local_irq(interrupt_flag);
        return result;
    }

    /// Fork current thread and create new thread.
    pub fn create_kernel_thread(
        &mut self,
        entry_address: fn() -> !,
        stack_size: Option<MSize>,
        priority_level: i8,
        should_set_into_run_queue: bool,
    ) -> Result<&'static mut ThreadEntry, TaskError> {
        let _lock = self.lock.lock();
        let mut clone_thread = get_cpu_manager_cluster()
            .run_queue_manager
            .copy_running_thread_data()?;
        let forked_thread =
            self.fork_system_thread(&mut clone_thread, entry_address, stack_size)?;
        let _forked_thread_lock = forked_thread.lock.lock();
        forked_thread.set_priority_level(priority_level);
        forked_thread.set_task_status(TaskStatus::New);

        if should_set_into_run_queue {
            if let Err(e) = get_cpu_manager_cluster()
                .run_queue_manager
                .add_thread(forked_thread)
            {
                pr_err!("Cannot add thread to RunQueueManager. Error: {:?}", e);
                let process = forked_thread.get_process_mut();
                if let Err(r_e) = process.remove_thread(forked_thread) {
                    pr_err!("Removing thread from process was failed. Error: {:?}", r_e);
                }
            }
        }
        return Ok(forked_thread);
    }

    pub fn get_context_manager(&self) -> &ContextManager {
        &self.context_manager
    }

    /// Set `thread` to `RunQueueManager`.
    ///
    /// This function sets `thread` to `RunQueueManager`(it may different cpu's).
    /// This does not check `ThreadEntry::sleep_list`.
    ///
    /// `thread` must be unlocked.
    pub fn wake_up_thread(&mut self, thread: &mut ThreadEntry) -> Result<(), TaskError> {
        let _thread_lock = thread.lock.lock();
        thread.set_ptr_to_list();
        thread.run_list.unset_prev_and_next();
        thread.set_task_status(TaskStatus::CanRun);
        /* Currently, add this cpu's run queue. */
        get_cpu_manager_cluster()
            .run_queue_manager
            .add_thread(thread)?;
        return Ok(());
    }
}
