//!
//! Task Manager
//!
//! This manager is the frontend of task management system.
//! The task management system has two structs, arch-independent and depends on arch.

mod process_entry;
pub mod run_queue;
mod scheduling_class;
mod thread_entry;
pub mod wait_queue;
pub mod work_queue;

pub use self::process_entry::ProcessEntry;
use self::run_queue::RunQueue;
use self::scheduling_class::{SchedulingClass, kernel::KernelSchedulingClass};
pub use self::thread_entry::ThreadEntry;

use crate::arch::target_arch::context::{ContextManager, context_data::ContextData};

use crate::kernel::{
    collections::ptr_linked_list::PtrLinkedList,
    manager_cluster::{CpuManagerCluster, get_cpu_manager_cluster, get_kernel_manager_cluster},
    memory_manager::{
        MemoryError, MemoryManager, kfree, kmalloc,
        {
            data_type::{MSize, VAddress},
            slab_allocator::GlobalSlabAllocator,
        },
    },
    sync::spin_lock::IrqSaveSpinLockFlag,
    task_manager::scheduling_class::user::UserSchedulingClass,
};

use core::mem::offset_of;
use core::ptr::null_mut;

pub const KERNEL_PID: usize = 0;

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

macro_rules! allocate_new_thread {
    ($s:expr) => {
        $s.thread_entry_pool
            .alloc()
            .map(|mut e| unsafe { &mut *e.as_mut() })
    };
}

macro_rules! allocate_new_process {
    ($s:expr) => {
        $s.process_entry_pool
            .alloc()
            .map(|mut e| unsafe { &mut *e.as_mut() })
    };
}

macro_rules! free_thread {
    ($s:expr,$t:expr) => {
        unsafe { core::ptr::drop_in_place($t) };
        $s.thread_entry_pool
            .free(core::ptr::NonNull::new($t).unwrap())
    };
}

macro_rules! free_process {
    ($s:expr,$p:expr) => {
        unsafe { core::ptr::drop_in_place($p) };
        $s.process_entry_pool
            .free(core::ptr::NonNull::new($p).unwrap())
    };
}

impl TaskManager {
    pub const FLAG_LOCAL_THREAD: u8 = 1;

    pub const fn new() -> Self {
        Self {
            lock: IrqSaveSpinLockFlag::new(),
            kernel_process: null_mut(),
            idle_thread: null_mut(),
            context_manager: ContextManager::new(),
            process_entry_pool: GlobalSlabAllocator::new(),
            thread_entry_pool: GlobalSlabAllocator::new(),
            p_list: PtrLinkedList::new(),
            next_process_id: 1,
        }
    }

    /// Init TaskManager
    ///
    /// This function setups memory pools and creates a kernel process.
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
        let kernel_process = allocate_new_process!(self).unwrap();
        let main_thread = allocate_new_thread!(self).unwrap();
        let idle_thread = allocate_new_thread!(self).unwrap();

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

        let memory_manager = &mut get_kernel_manager_cluster().kernel_memory_manager;

        kernel_process.init(
            KERNEL_PID,
            null_mut(),
            &mut [main_thread, idle_thread],
            memory_manager, /* should we set null? */
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
        run_queue.set_idle_thread(idle_thread);
        drop(_idle_thread_lock);
        self.kernel_process = kernel_process;
        self.idle_thread = idle_thread;
        drop(_lock);
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
        run_queue.set_idle_thread(forked_thread);
        drop(_lock);
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
        let new_thread = allocate_new_thread!(self)?;
        let _original_thread_lock = thread.lock.lock();
        let new_context = self.context_manager.fork_system_context(
            thread.get_context(),
            entry_address,
            stack_size,
        )?;
        new_thread.fork_data(thread, new_context);
        drop(_original_thread_lock);
        let parent_process = unsafe { &mut *new_thread.get_process_mut() };
        let _process_lock = parent_process
            .lock
            .try_lock()
            .or(Err(TaskError::ThreadLockError))?;
        parent_process.add_thread(new_thread)?;
        Ok(new_thread)
    }

    /// Create kernel thread and set into a kernel process.
    ///
    /// This function forks `Self::idle_thread` and returns it.
    /// This will be used to make threads before RunQueue starts.
    pub fn create_kernel_thread_for_init(
        &mut self,
        entry_address: fn() -> !,
        stack_size: Option<MSize>,
        kernel_priority: u8,
        flag: u8,
    ) -> Result<&'static mut ThreadEntry, TaskError> {
        let _lock = self.lock.lock();
        let result = try {
            let main_thread = unsafe { &mut *self.idle_thread };
            let forked_thread = self.fork_system_thread(main_thread, entry_address, stack_size)?;
            let _forked_thread_lock = forked_thread.lock.lock();
            forked_thread
                .set_priority_level(KernelSchedulingClass::get_custom_priority(kernel_priority));
            forked_thread.set_task_status(TaskStatus::New);
            if (flag & Self::FLAG_LOCAL_THREAD) != 0 {
                forked_thread.set_local_thread();
            }
            forked_thread
        };

        drop(_lock);
        result
    }

    /// Fork current thread and create new thread.
    ///
    /// This function forks the current thread and adds into RunQueue if needed.
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

        if should_set_into_run_queue
            && let Err(err) = get_cpu_manager_cluster()
                .run_queue
                .add_thread(forked_thread)
        {
            pr_err!("Cannot add thread to RunQueue. Error: {:?}", err);
            let process = unsafe { &mut *forked_thread.get_process_mut() };
            bug_on_err!(process.remove_thread(forked_thread));
        }
        Ok(forked_thread)
    }

    /// Fork current thread and create new thread with an argument
    ///
    /// This function forks the current thread and adds into RunQueue if needed.
    /// This function assumes RunQueue::running_thread.is_some() == TRUE.
    pub fn create_kernel_thread_with_argument(
        &mut self,
        entry_address: fn(argument: usize) -> !,
        stack_size: Option<MSize>,
        kernel_priority: u8,
        argument: usize,
    ) -> Result<&'static mut ThreadEntry, TaskError> {
        let thread = self.create_kernel_thread(
            unsafe { core::mem::transmute::<fn(usize) -> !, fn() -> !>(entry_address) },
            stack_size,
            kernel_priority,
            false,
        )?;
        let _thread_lock = thread.lock.lock();
        thread
            .get_context()
            .set_function_call_arguments(&[argument as u64]);
        drop(_thread_lock);
        Ok(thread)
    }

    pub fn create_user_process(
        &mut self,
        parent_process: Option<&mut ProcessEntry>,
        privilege_level: u8,
    ) -> Result<&'static mut ProcessEntry, TaskError> {
        /* Create Memory Manager */
        let user_memory_manager = match kmalloc!(
            MemoryManager,
            get_kernel_manager_cluster()
                .kernel_memory_manager
                .create_user_memory_manager()?
        ) {
            Ok(m) => m,
            Err(err) => {
                pr_err!("Failed to allocate MemoryManager: {:?}", err);
                return Err(TaskError::MemoryError(err));
            }
        };

        let _lock = self.lock.lock();
        let result = try {
            let new_process = allocate_new_process!(self)?;
            let parent = match parent_process {
                Some(p) => p as *mut _,
                None => {
                    assert_eq!(self.next_process_id, 1);
                    null_mut()
                }
            };
            new_process.init(
                self.next_process_id,
                parent,
                &mut [],
                user_memory_manager as *mut _,
                privilege_level,
            );
            unsafe { self.p_list.insert_tail(&mut new_process.p_list) };
            self.update_next_p_id();
            new_process
        };

        drop(_lock);
        if let Err(err) = &result {
            pr_err!("Failed to create a process for user: {:?}", err);
            bug_on_err!(user_memory_manager.free_all_allocated_memory());
            bug_on_err!(kfree!(user_memory_manager));
        }
        result
    }

    pub fn create_user_thread(
        &mut self,
        process: &mut ProcessEntry,
        entry_address: usize,
        arguments: &[usize],
        stack_address: VAddress,
        priority_level: u8,
    ) -> Result<&mut ThreadEntry, TaskError> {
        assert_ne!(process.get_pid(), 0);
        let _lock = self.lock.lock();
        let result = try {
            let new_thread = allocate_new_thread!(self)?;
            let context_data = match self.get_context_manager().create_user_context(
                entry_address,
                stack_address,
                arguments,
            ) {
                Ok(c) => c,
                Err(err) => {
                    pr_err!("Failed to create thread context: {:?}", err);
                    free_thread!(self, new_thread);
                    return Err(TaskError::MemoryError(err));
                }
            };

            new_thread.init(
                process,
                priority_level,
                SchedulingClass::UserThread(UserSchedulingClass::new()),
                context_data,
            );
            new_thread.set_priority_level(UserSchedulingClass::get_custom_priority(priority_level));
            new_thread.set_task_status(TaskStatus::New);
            let _process_lock = process.lock.lock();
            if let Err(err) = process.add_thread(new_thread) {
                pr_err!("Failed to add a thread into the process: {:?}", err);
                free_thread!(self, new_thread);
                return Err(err);
            }
            new_thread
        };
        drop(_lock);
        bug_on_err!(&result);
        result
    }

    pub fn delete_user_process(
        &mut self,
        target_process: &mut ProcessEntry,
    ) -> Result<(), TaskError> {
        let mut _lock = Some(target_process.lock.lock());

        /* Delete all children */
        for e in unsafe {
            target_process
                .children
                .iter_mut(offset_of!(ProcessEntry, siblings))
        } {
            _lock = None;
            self.delete_user_process(e)?;
            _lock = Some(target_process.lock.lock());
        }

        if target_process.get_process_status() != ProcessStatus::Zombie {
            pr_err!(
                "Invalid Task Status: {:?}",
                target_process.get_process_status()
            );
            /* TODO: send SIGTERM */
            return Err(TaskError::InvalidProcessEntry);
        }

        /* Delete all threads */
        while let Some(thread) = target_process.take_thread()? {
            let _lock = thread.lock.lock();
            if thread.get_task_status() != TaskStatus::Stopped {
                pr_err!("Thread is not stopped.");
                return Err(TaskError::InvalidProcessEntry);
            }
            free_thread!(self, thread);
        }

        /* Delete from parent */
        if let Some(mut parent) = target_process.get_parent_process() {
            let parent = unsafe { parent.as_mut() };
            let mut _parent_lock;
            loop {
                if let Ok(e) = parent.lock.try_lock() {
                    if _lock.is_none() {
                        if let Ok(l) = target_process.lock.try_lock() {
                            _lock = Some(l);
                        } else {
                            drop(e);
                            continue;
                        }
                    }
                    _parent_lock = e;
                    break;
                }
                _lock = None;
            }
            unsafe { parent.children.remove(&mut target_process.siblings) };
        }

        /* Delete Memory Manager */
        let memory_manager = unsafe { &mut *target_process.get_memory_manager() };
        memory_manager.free_all_allocated_memory()?;
        let _ = kfree!(memory_manager);
        let _self_lock = self.lock.lock();
        unsafe { self.p_list.remove(&mut target_process.p_list) };
        drop(_lock);
        free_process!(self, target_process);
        drop(_self_lock);
        Ok(())
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
        if !thread.is_local_thread() {
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
        }

        /* Add into Current CPU */

        get_cpu_manager_cluster().run_queue.add_thread(thread)
    }

    /// Set `thread` to `RunQueueManager`.
    ///
    /// This function sets `thread` to `RunQueueManager`(it may be different CPU's one).
    /// This does not check `ThreadEntry::sleep_list`.
    ///
    /// `thread` must be unlocked.
    pub fn wake_up_thread(&mut self, thread: &mut ThreadEntry) -> Result<(), TaskError> {
        let _lock = self.lock.lock();
        let result = self.add_thread_into_run_queue(thread);
        drop(_lock);
        result
    }
}
