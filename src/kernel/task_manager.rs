//!
//! Task Manager
//!
//! This manager is the frontend of task management system.
//! Task management system has two struct, arch-independent and depend on arch.

mod process_entry;
pub mod run_queue;
mod scheduling_class;
mod thread_entry;
pub mod wait_queue;
pub mod work_queue;

use self::process_entry::ProcessEntry;
use self::run_queue::RunQueue;
use self::scheduling_class::{kernel::KernelSchedulingClass, SchedulingClass};
use self::thread_entry::ThreadEntry;

use crate::arch::target_arch::context::{context_data::ContextData, ContextManager};

use crate::kernel::collections::ptr_linked_list::PtrLinkedList;
use crate::kernel::manager_cluster::{
    get_cpu_manager_cluster, get_kernel_manager_cluster, CpuManagerCluster,
};
use crate::kernel::memory_manager::data_type::{Address, MSize};
use crate::kernel::memory_manager::slab_allocator::GlobalSlabAllocator;
use crate::kernel::memory_manager::{MemoryError, MemoryManager};
use crate::kernel::sync::spin_lock::IrqSaveSpinLockFlag;
use crate::kernel::task_manager::scheduling_class::user::UserSchedulingClass;

pub struct TaskManager {
    lock: IrqSaveSpinLockFlag,
    kernel_process: *mut ProcessEntry,
    idle_thread: *mut ThreadEntry,
    context_manager: ContextManager,
    process_entry_pool: GlobalSlabAllocator<ProcessEntry>,
    thread_entry_pool: GlobalSlabAllocator<ThreadEntry>,
    p_list: PtrLinkedList<ProcessEntry>,
    next_process_id: usize,
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
    Interruptible,
    Uninterruptible,
    Running,
    Stopped,
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
    pub const fn new() -> Self {
        Self {
            lock: IrqSaveSpinLockFlag::new(),
            kernel_process: core::ptr::null_mut(),
            idle_thread: core::ptr::null_mut(),
            context_manager: ContextManager::new(),
            process_entry_pool: GlobalSlabAllocator::new(ProcessEntry::PROCESS_ENTRY_ALIGN),
            thread_entry_pool: GlobalSlabAllocator::new(ThreadEntry::THREAD_ENTRY_ALIGN),
            p_list: PtrLinkedList::new(),
            next_process_id: 1,
        }
    }

    /// Init TaskManager
    ///
    /// This function setups memory pools and create kernel process.
    /// The kernel process has two threads created with kernel_main_context and idle_context.
    /// After that, this function will set those threads into run_queue_manager.
    pub fn init(
        &mut self,
        context_manager: ContextManager,
        kernel_main_context: ContextData,
        idle_context: ContextData,
        run_queue: &mut RunQueue,
    ) {
        let _lock = self.lock.lock();
        self.context_manager = context_manager;

        self.process_entry_pool
            .init()
            .expect("Failed to init the ProcessEntryPool");
        self.thread_entry_pool
            .init()
            .expect("Failed to init the ThreadEntryPool");

        /* Create the kernel process and threads */
        let kernel_process = self.process_entry_pool.alloc().unwrap();
        let main_thread = self.thread_entry_pool.alloc().unwrap();
        let idle_thread = self.thread_entry_pool.alloc().unwrap();

        main_thread.init(
            kernel_process,
            KernelSchedulingClass::get_normal_priority(),
            SchedulingClass::KernelThread(KernelSchedulingClass::new()),
            kernel_main_context,
        );
        idle_thread.init(
            kernel_process,
            KernelSchedulingClass::get_idle_thread_priority(),
            SchedulingClass::KernelThread(KernelSchedulingClass::new()),
            idle_context,
        );

        let memory_manager = &mut get_kernel_manager_cluster().memory_manager;

        kernel_process.init(
            0,
            core::ptr::null_mut(),
            &mut [main_thread, idle_thread],
            memory_manager,
            0,
        );

        let _process_lock = kernel_process.lock.lock();
        let _main_thread_lock = main_thread.lock.lock();
        let _idle_thread_lock = idle_thread.lock.lock();

        /* Set threads to run_queue_manager */
        run_queue
            .add_thread(main_thread)
            .expect("Cannot add main thread to RunQueue");
        drop(_main_thread_lock);
        run_queue
            .add_thread(idle_thread)
            .expect("Cannot add idle thread to RunQueue");
        drop(_idle_thread_lock);
        self.kernel_process = kernel_process;
        self.idle_thread = idle_thread;
        drop(_lock);
        return;
    }

    /// Init idle thread for additional processors.
    ///
    /// This function forks idle thread and sets it to run_queue_manager.
    /// This will be used for application processors' initialization.
    pub fn init_idle(&mut self, idle_fn: fn() -> !, run_queue: &mut RunQueue) {
        let _lock = self.lock.lock();
        let idle_thread = unsafe { &mut *self.idle_thread };
        let forked_thread = self
            .fork_system_thread(
                idle_thread,
                idle_fn,
                Some(ContextManager::IDLE_THREAD_STACK_SIZE),
            )
            .expect("Cannot fork idle thread");
        let _lock = forked_thread.lock.lock();
        run_queue
            .add_thread(forked_thread)
            .expect("Cannot init ap's idle thread");
        drop(_lock);
        return;
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
        assert!(self.lock.is_locked());
        let new_thread = self.thread_entry_pool.alloc()?;
        let _original_thread_lock = thread.lock.lock();
        let new_context = self.context_manager.fork_system_context(
            thread.get_context(),
            entry_address,
            stack_size,
        )?;
        new_thread.fork_data(&thread, new_context);
        drop(_original_thread_lock);
        let parent_process = new_thread.get_process_mut();
        let _process_lock = parent_process
            .lock
            .try_lock()
            .or(Err(TaskError::ThreadLockError))?;
        parent_process.add_thread(new_thread)?;
        return Ok(new_thread);
    }

    /// Create kernel thread and set into kernel process.
    ///
    /// This function forks `Self::idle_thread` and returns it.
    /// This will be used to make threads before RunQueue starts.
    pub fn create_kernel_thread_for_init(
        &mut self,
        entry_address: fn() -> !,
        stack_size: Option<MSize>,
        kernel_priority: u8,
    ) -> Result<&'static mut ThreadEntry, TaskError> {
        let _lock = self.lock.lock();
        let result = try {
            let mut main_thread = unsafe { &mut *self.idle_thread };
            let forked_thread =
                self.fork_system_thread(&mut main_thread, entry_address, stack_size)?;
            let _forked_thread_lock = forked_thread.lock.lock();
            forked_thread
                .set_priority_level(KernelSchedulingClass::get_custom_priority(kernel_priority));
            forked_thread.set_task_status(TaskStatus::New);
            forked_thread
        };

        drop(_lock);
        return result;
    }

    /// Fork current thread and create new thread.
    ///
    /// This function forks current thread and adds into RunQueue if needed.
    /// This function assumes RunQueue::running_thread.is_some() == TRUE.
    pub fn create_kernel_thread(
        &mut self,
        entry_address: fn() -> !,
        stack_size: Option<MSize>,
        kernel_priority: u8,
        should_set_into_run_queue: bool,
    ) -> Result<&'static mut ThreadEntry, TaskError> {
        let _lock = self.lock.lock();
        let mut clone_thread = get_cpu_manager_cluster()
            .run_queue
            .copy_running_thread_data()?;
        let forked_thread =
            self.fork_system_thread(&mut clone_thread, entry_address, stack_size)?;
        let _forked_thread_lock = forked_thread.lock.lock();
        forked_thread
            .set_priority_level(KernelSchedulingClass::get_custom_priority(kernel_priority));
        forked_thread.set_task_status(TaskStatus::New);
        forked_thread.time_slice = 5; /* Temporary */

        if should_set_into_run_queue {
            if let Err(e) = get_cpu_manager_cluster()
                .run_queue
                .add_thread(forked_thread)
            {
                pr_err!("Cannot add thread to RunQueue. Error: {:?}", e);
                let process = forked_thread.get_process_mut();
                if let Err(r_e) = process.remove_thread(forked_thread) {
                    pr_err!("Removing thread from process was failed. Error: {:?}", r_e);
                }
            }
        }
        return Ok(forked_thread);
    }

    pub fn create_user_process(
        &mut self,
        parent_process: *mut ProcessEntry,
        privilege_level: u8,
    ) -> Result<&'static mut ProcessEntry, TaskError> {
        /* Create Memory Manager */
        let mut user_memory_manger = get_kernel_manager_cluster()
            .memory_manager
            .create_user_memory_manager()?;
        const MEMORY_MANAGER_SIZE: MSize = MSize::new(core::mem::size_of::<MemoryManager>());
        let allocated_user_memory_manager_address = get_cpu_manager_cluster()
            .memory_allocator
            .kmalloc(MEMORY_MANAGER_SIZE);
        if let Err(e) = allocated_user_memory_manager_address {
            pr_err!("Failed to allocate MemoryManager: {:?}", e);
            user_memory_manger.disable();
            return Err(TaskError::MemoryError(e));
        }
        let allocated_user_memory_manager = unsafe {
            &mut *(allocated_user_memory_manager_address.unwrap().to_usize() as *mut MemoryManager)
        };
        core::mem::forget(core::mem::replace(
            allocated_user_memory_manager,
            user_memory_manger,
        ));

        let _lock = self.lock.lock();
        let result = try {
            let new_process = self.process_entry_pool.alloc()?;
            if parent_process.is_null() {
                assert_eq!(self.next_process_id, 1);
            }
            new_process.init(
                self.next_process_id,
                parent_process,
                &mut [],
                allocated_user_memory_manager as *mut _,
                privilege_level,
            );
            self.p_list.insert_tail(&mut new_process.p_list);
            self.update_next_p_id();
            new_process
        };

        drop(_lock);
        if let Err(e) = &result {
            pr_err!("Failed to create a process for user: {:?}", e);
            allocated_user_memory_manager.disable();
            if let Err(e) = get_cpu_manager_cluster().memory_allocator.kfree(
                allocated_user_memory_manager_address.unwrap(),
                MEMORY_MANAGER_SIZE,
            ) {
                pr_err!("Failed to free the MemoryManager: {:?}", e);
            }
        }
        return result;
    }

    pub fn create_user_thread(
        &mut self,
        process: &mut ProcessEntry,
        entry_address: fn() -> !,
        default_stack_size: Option<MSize>,
        priority_level: u8,
    ) -> Result<&mut ThreadEntry, TaskError> {
        assert_ne!(process.get_pid(), 0);
        /* TODO: Lazy allocation*/
        let stack_size =
            default_stack_size.unwrap_or(MSize::new(ContextManager::DEFAULT_STACK_SIZE_OF_USER));
        let stack_address = get_cpu_manager_cluster()
            .memory_allocator
            .kmalloc(stack_size);
        if let Err(e) = stack_address {
            pr_err!("Failed to allocate Stack for user: {:?}", e);
            return Err(TaskError::MemoryError(e));
        }
        let stack_start_address = stack_address.unwrap() + stack_size;
        let _lock = self.lock.lock();
        let result = try {
            let new_thread = self.thread_entry_pool.alloc()?;
            let context_data = self
                .get_context_manager()
                .create_user_context(entry_address, stack_start_address);
            if let Err(e) = context_data {
                pr_err!("Failed to create thread context: {:?}", e);
                self.thread_entry_pool.free(new_thread);
                Err(TaskError::MemoryError(e))?;
                unreachable!() /* To avoid compile error */
            }

            new_thread.init(
                process,
                priority_level,
                SchedulingClass::UserThread(UserSchedulingClass::new()),
                context_data.unwrap(),
            );
            new_thread.set_priority_level(UserSchedulingClass::get_custom_priority(priority_level));
            new_thread.set_task_status(TaskStatus::New);
            let _process_lock = process.lock.lock();
            if let Err(e) = process.add_thread(new_thread) {
                pr_err!("Failed to add a thread into the process: {:?}", e);
                self.thread_entry_pool.free(new_thread);
                Err(e)?;
                unreachable!() /* To avoid compile error */
            }
            new_thread
        };
        drop(_lock);
        if let Err(e) = &result {
            pr_err!("Failed to create a thread for user: {:?}", e);
            if let Err(e) = get_cpu_manager_cluster()
                .memory_allocator
                .kfree(stack_address.unwrap(), stack_size)
            {
                pr_err!("Failed to free the stack: {:?}", e);
            }
        }
        return result;
    }

    pub fn get_context_manager(&self) -> &ContextManager {
        &self.context_manager
    }

    fn update_next_p_id(&mut self) {
        assert!(self.next_process_id <= usize::MAX);
        self.next_process_id += 1;
    }

    /// Add thread into RunQueue with checking each CPU's load.
    ///
    /// `thread` must be unlocked.
    fn add_thread_into_run_queue(&self, thread: &mut ThreadEntry) -> Result<(), TaskError> {
        assert!(self.lock.is_locked());
        let _thread_lock = thread.lock.lock();
        let current_cpu_load = get_cpu_manager_cluster()
            .run_queue
            .get_number_of_running_threads();
        for cpu in unsafe {
            get_kernel_manager_cluster()
                .cpu_list
                .iter_mut(offset_of!(CpuManagerCluster, list))
        } {
            let load = cpu.run_queue.get_number_of_running_threads();
            if load < current_cpu_load {
                let should_interrupt_cpu = cpu.run_queue.assign_thread(thread)?;
                drop(_thread_lock);
                if should_interrupt_cpu {
                    get_cpu_manager_cluster()
                        .interrupt_manager
                        .send_reschedule_ipi(cpu.cpu_id);
                }
                return Ok(());
            }
        }

        /* Add into Current CPU */
        let result = get_cpu_manager_cluster().run_queue.add_thread(thread);
        return result;
    }

    /// Set `thread` to `RunQueueManager`.
    ///
    /// This function sets `thread` to `RunQueueManager`(it may different cpu's).
    /// This does not check `ThreadEntry::sleep_list`.
    ///
    /// `thread` must be unlocked.
    pub fn wake_up_thread(&mut self, thread: &mut ThreadEntry) -> Result<(), TaskError> {
        let _lock = self.lock.lock();
        let result = self.add_thread_into_run_queue(thread);
        drop(_lock);
        return result;
    }
}
