//!
//! Task Manager
//!
//! This manager is the frontend of task management system.
//! Task management system has two struct, arch-independent and depend on arch.

mod process_entry;
pub mod run_queue_manager;
mod thread_entry;
pub mod work_queue;

use self::process_entry::ProcessEntry;
use self::thread_entry::ThreadEntry;

use crate::arch::target_arch::context::{context_data::ContextData, ContextManager};
use crate::arch::target_arch::interrupt::InterruptManager;

use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::data_type::MSize;
use crate::kernel::memory_manager::object_allocator::cache_allocator::CacheAllocator;
use crate::kernel::ptr_linked_list::PtrLinkedList;
use crate::kernel::sync::spin_lock::SpinLockFlag;

pub struct TaskManager {
    lock: SpinLockFlag,
    kernel_process: Option<*mut ProcessEntry>,
    sleep_list: PtrLinkedList<ThreadEntry>,
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

impl TaskManager {
    const NUM_OF_INITIAL_THREAD_ENTRIES: usize = 6;
    const NUM_OF_INITIAL_PROCESS_ENTRIES: usize = 6;
    pub const fn new() -> Self {
        Self {
            lock: SpinLockFlag::new(),
            sleep_list: PtrLinkedList::new(),
            kernel_process: None,
            context_manager: ContextManager::new(),
            process_entry_pool: CacheAllocator::new(ProcessEntry::PROCESS_ENTRY_ALIGN_ORDER),
            thread_entry_pool: CacheAllocator::new(ThreadEntry::THREAD_ENTRY_ALIGN_ORDER),
        }
    }

    /// Init TaskManager
    ///
    /// This function setups memory pools and create kernel process.
    /// The kernel process has one thread created with main_context.
    /// The return value is the created thread.
    pub fn init(
        &mut self,
        context_manager: ContextManager,
        main_context: ContextData,
    ) -> &'static mut ThreadEntry {
        let _lock = self.lock.lock();
        let memory_manager = &get_kernel_manager_cluster().memory_manager;
        self.context_manager = context_manager;

        if let Err(e) = self.process_entry_pool.init(
            Self::NUM_OF_INITIAL_PROCESS_ENTRIES,
            &mut memory_manager.lock().unwrap(),
        ) {
            pr_err!("Allocating the pool was failed: {:?}", e);
        }
        if let Err(e) = self.thread_entry_pool.init(
            Self::NUM_OF_INITIAL_THREAD_ENTRIES,
            &mut memory_manager.lock().unwrap(),
        ) {
            pr_err!("Allocating the pool was failed: {:?}", e);
        }

        /* Create the kernel process and thread */
        let kernel_process = self.process_entry_pool.alloc(Some(memory_manager)).unwrap();
        let main_thread = self.thread_entry_pool.alloc(Some(memory_manager)).unwrap();
        main_thread.init(kernel_process, 0, 0, main_context);
        kernel_process.init_kernel_process(&mut [main_thread], memory_manager as *const _, 0);
        self.kernel_process = Some(kernel_process);
        main_thread
    }

    /// Create Kernel Process(pid: 0)
    ///
    /// This function creates kernel process and two threads(main and idle).
    /// They will be set into current CPU's RunQueueManager.
    /// This should be called by the boot strap processor.
    pub fn create_kernel_process(
        &mut self,
        context_for_main: ContextData,
        context_for_idle: ContextData,
    ) {
        let _lock = self.lock.lock();

        let memory_manager = &get_kernel_manager_cluster().memory_manager;
        let process_entry = self.process_entry_pool.alloc(Some(memory_manager)).unwrap();
        let main_thread = self.thread_entry_pool.alloc(Some(memory_manager)).unwrap();
        let idle_thread = self.thread_entry_pool.alloc(Some(memory_manager)).unwrap();

        main_thread.init(process_entry, 0, 0, context_for_main);
        idle_thread.init(process_entry, 0, core::i8::MIN, context_for_idle);

        process_entry.init_kernel_process(
            &mut [main_thread, idle_thread],
            memory_manager as *const _,
            0,
        );
        self.kernel_process = Some(process_entry as *mut _);

        let run_queue_manager = &mut get_kernel_manager_cluster()
            .boot_strap_cpu_manager
            .run_queue_manager;
        run_queue_manager.add_thread(main_thread);
        run_queue_manager.add_thread(idle_thread);
    }

    fn insert_into_sleep_list(
        &mut self,
        thread: &mut ThreadEntry,
        run_list: &mut PtrLinkedList<ThreadEntry>,
    ) {
        let _lock = self.lock.lock();
        thread.insert_self_to_sleep_queue(&mut self.sleep_list, run_list);
    }

    pub fn wakeup(&mut self, p_id: usize, t_id: usize) {
        let flag = InterruptManager::save_and_disable_local_irq();
        let _lock = self.lock.lock();
        for e in self.sleep_list.iter_mut() {
            let e = unsafe { &mut *e };
            let e_p_id = e.get_process().get_pid();
            let e_t_id = e.get_t_id();
            if e_p_id == p_id && e_t_id == t_id {
                if e.get_task_status() == TaskStatus::Sleeping {
                    let run_queue_manager = &mut get_cpu_manager_cluster().run_queue_manager;
                    run_queue_manager.wakeup(e, &mut self.sleep_list);
                }
                InterruptManager::restore_local_irq(flag);
                return;
            }
        }
        drop(_lock);
        InterruptManager::restore_local_irq(flag);
        //pr_err!("There is no thread to wakeup.");
    }

    fn wakeup_target_thread(&mut self, thread: &mut ThreadEntry) -> bool {
        if thread.get_task_status() != TaskStatus::Sleeping {
            return true;
        }
        let flag = InterruptManager::save_and_disable_local_irq();
        let _lock = if let Ok(l) = self.lock.try_lock() {
            l
        } else {
            InterruptManager::restore_local_irq(flag);
            return false;
        };
        let run_queue_manager = &mut get_cpu_manager_cluster().run_queue_manager;
        run_queue_manager.wakeup(thread, &mut self.sleep_list);
        drop(_lock);
        InterruptManager::restore_local_irq(flag);
        return true;
    }

    pub fn get_context_manager(&self) -> &ContextManager {
        &self.context_manager
    }

    pub fn create_kernel_thread(
        &mut self,
        entry: *const fn() -> !,
        stack_size: Option<MSize>,
        priority_level: i8,
    ) -> Result<&'static mut ThreadEntry, ()> {
        if self.kernel_process.is_none() {
            return Err(());
        }
        let _lock = self.lock.lock();
        let kernel_context = self
            .context_manager
            .create_system_context(entry as usize, stack_size);
        if kernel_context.is_err() {
            return Err(());
        }

        let thread = self
            .thread_entry_pool
            .alloc(Some(&get_kernel_manager_cluster().memory_manager));
        if thread.is_err() {
            return Err(());
        }
        drop(_lock);

        let thread = thread.unwrap();
        thread.init(
            self.kernel_process.unwrap(),
            0,
            priority_level,
            kernel_context.unwrap(),
        );
        thread.set_task_status(TaskStatus::New);
        let _lock = self.lock.lock();
        unsafe { &mut *self.kernel_process.unwrap() }.add_thread(thread);
        Ok(thread)
    }

    fn search_process_mut(&mut self, p_id: usize) -> Option<*mut ProcessEntry> {
        /* Assume locked */
        assert_ne!(p_id, 0);

        let process = self.kernel_process;
        if process.is_none() {
            return None;
        }
        let mut process = process.unwrap();
        while let Some(p) = unsafe { &mut *process }.get_next_process_from_p_list_mut() {
            if unsafe { &*p }.get_pid() == p_id {
                return Some(p);
            }
            process = p;
        }
        None
    }
}
