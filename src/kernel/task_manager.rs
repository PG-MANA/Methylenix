/*
 * Task Manager
 * This manager is the frontend of task management system.
 * Task management system has two struct, arch-independent and depend on arch.
 */

mod process_entry;
mod thread_entry;

use self::process_entry::ProcessEntry;
use self::thread_entry::ThreadEntry;

use crate::arch::target_arch::context::{context_data::ContextData, ContextManager};

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::object_allocator::cache_allocator::CacheAllocator;
use crate::kernel::ptr_linked_list::PtrLinkedList;
use crate::kernel::sync::spin_lock::SpinLockFlag;

pub struct TaskManager {
    lock: SpinLockFlag,
    run_list: PtrLinkedList<ThreadEntry>,
    sleep_list: PtrLinkedList<ThreadEntry>,
    running_thread: Option<*mut ThreadEntry>,
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
    Zombie,
}

impl TaskManager {
    const NUM_OF_INITIAL_THREAD_ENTRIES: usize = 6;
    const NUM_OF_INITIAL_PROCESS_ENTRIES: usize = 6;
    pub const fn new() -> Self {
        Self {
            lock: SpinLockFlag::new(),
            run_list: PtrLinkedList::new(),
            sleep_list: PtrLinkedList::new(),
            running_thread: None,
            context_manager: ContextManager::new(),
            process_entry_pool: CacheAllocator::new(ProcessEntry::PROCESS_ENTRY_ALIGN_ORDER),
            thread_entry_pool: CacheAllocator::new(ThreadEntry::THREAD_ENTRY_ALIGN_ORDER),
        }
    }

    pub fn init(&mut self, context_manager: ContextManager) {
        let _lock = self.lock.lock();
        let memory_manager = &get_kernel_manager_cluster().memory_manager;

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

        self.context_manager = context_manager;
    }

    pub fn create_init_process(
        &mut self,
        context_for_init: ContextData,
        context_for_idle: ContextData,
    ) {
        let _lock = self.lock.lock();

        let memory_manager = &get_kernel_manager_cluster().memory_manager;
        let process_entry = self.process_entry_pool.alloc(Some(memory_manager)).unwrap();
        let thread_entry = self.thread_entry_pool.alloc(Some(memory_manager)).unwrap();
        let idle_thread_entry = self.thread_entry_pool.alloc(Some(memory_manager)).unwrap();

        thread_entry.init(1, process_entry, 0, 0, context_for_init);
        idle_thread_entry.init(2, process_entry, 0, core::i8::MIN, context_for_idle);

        process_entry.init(1, thread_entry, 0, 0);
        process_entry.add_thread(idle_thread_entry);

        thread_entry.set_up_to_be_root_of_run_list(&mut self.run_list);
        thread_entry.insert_after_of_run_list(idle_thread_entry);
    }

    pub fn execute_init_process(&mut self) -> ! {
        let _lock = self.lock.lock();
        let init_thread = unsafe { self.run_list.get_first_entry_mut().unwrap() };
        assert_eq!(init_thread.get_process().get_pid(), 1);
        self.running_thread = Some(init_thread);
        init_thread.set_task_status(TaskStatus::Running);
        drop(_lock);
        unsafe {
            self.context_manager
                .jump_to_context(init_thread.get_context())
        };
        /* not return here. */
        panic!("Switching to init process was failed.");
    }

    pub fn switch_to_next_thread(&mut self) {
        let _lock = self.lock.lock();
        let running_thread = unsafe { &mut *self.running_thread.unwrap() };
        let next_thread = if running_thread.get_task_status() == TaskStatus::Sleeping {
            let should_change_root = running_thread.is_root_of_run_list();
            let next_entry = running_thread.get_next_from_run_list_mut();
            running_thread.insert_self_to_sleep_queue(&mut self.sleep_list, &mut self.run_list);
            if should_change_root {
                /*assert!(next_entry.is_some());*/
                let next_entry = next_entry.unwrap();
                next_entry.set_up_to_be_root_of_run_list(&mut self.run_list);
                next_entry
            } else if let Some(next_entry) = next_entry {
                next_entry
            } else {
                unsafe { self.run_list.get_first_entry_mut().unwrap() }
            }
        } else {
            running_thread.set_task_status(TaskStatus::CanRun);
            if let Some(t) = running_thread.get_next_from_run_list_mut() {
                t
            } else {
                unsafe { self.run_list.get_first_entry_mut().unwrap() }
            }
        };
        pr_info!(
            "Task Switch[thread_id:{}=>{}]",
            running_thread.get_t_id(),
            next_thread.get_t_id(),
        );
        next_thread.set_task_status(TaskStatus::Running);
        self.running_thread = Some(next_thread as *mut _);
        drop(_lock);
        unsafe {
            self.context_manager
                .switch_context(running_thread.get_context(), next_thread.get_context());
        }
    }

    /* sleep running thread and switch to next thread */
    pub fn sleep(&mut self) {
        let _lock = self.lock.lock();
        let running_thread = unsafe { &mut *self.running_thread.unwrap() };
        running_thread.set_task_status(TaskStatus::Sleeping);
        drop(_lock);
        self.switch_to_next_thread(); /* running_thread will be linked in sleep_list in this function*/
        /* woke up and return */
    }

    pub fn wakeup(&mut self, p_id: usize, t_id: usize) {
        let _lock = self.lock.lock();
        for e in self.sleep_list.iter_mut() {
            let e = unsafe { &mut *e };
            let e_p_id = e.get_process().get_pid();
            let e_t_id = e.get_t_id();
            if e_p_id == p_id && e_t_id == t_id {
                if e.get_task_status() == TaskStatus::Sleeping {
                    e.wakeup(&mut self.run_list, &mut self.sleep_list);
                }
                return;
            }
        }
        pr_err!("There is no thread to wakeup.");
    }

    pub fn get_context_manager(&self) -> &ContextManager {
        &self.context_manager
    }
}
