/*
 * Task Manager
 * This manager is the frontend of task management system.
 * Task management system has two struct, arch-independent and depend on arch.
 */

mod process_entry;
mod thread_entry;
pub mod work_queue;

use self::process_entry::ProcessEntry;
use self::thread_entry::ThreadEntry;

use crate::arch::target_arch::context::{context_data::ContextData, ContextManager};
use crate::arch::target_arch::interrupt::InterruptManager;

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::MSize;
use crate::kernel::memory_manager::object_allocator::cache_allocator::CacheAllocator;
use crate::kernel::ptr_linked_list::PtrLinkedList;
use crate::kernel::sync::spin_lock::SpinLockFlag;

pub struct TaskManager {
    lock: SpinLockFlag,
    kernel_process: Option<*mut ProcessEntry>,
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
            run_list: PtrLinkedList::new(),
            sleep_list: PtrLinkedList::new(),
            running_thread: None,
            kernel_process: None,
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

    pub fn create_kernel_process(
        &mut self,
        context_for_main: ContextData,
        context_for_idle: ContextData,
    ) {
        let _lock = self.lock.lock();

        let memory_manager = &get_kernel_manager_cluster().memory_manager;
        let process_entry = self.process_entry_pool.alloc(Some(memory_manager)).unwrap();
        let main_thread = self.thread_entry_pool.alloc(Some(memory_manager)).unwrap();
        let idle_thread_entry = self.thread_entry_pool.alloc(Some(memory_manager)).unwrap();

        main_thread.init(process_entry, 0, 0, context_for_main);
        idle_thread_entry.init(process_entry, 0, core::i8::MIN, context_for_idle);

        process_entry.init_kernel_process(
            &mut [main_thread, idle_thread_entry],
            memory_manager as *const _,
            0,
        );
        self.kernel_process = Some(process_entry as *mut _);

        main_thread.set_up_to_be_root_of_run_list(&mut self.run_list);
        main_thread.insert_after_of_run_list(idle_thread_entry);
    }

    pub fn execute_kernel_process(&mut self) -> ! {
        let _lock = self.lock.lock();
        let main_thread = unsafe { self.run_list.get_first_entry_mut().unwrap() };
        assert_eq!(main_thread.get_process().get_pid(), 0);
        assert_eq!(main_thread.get_t_id(), 1);

        self.running_thread = Some(main_thread);
        main_thread.set_task_status(TaskStatus::Running);
        drop(_lock);
        unsafe {
            self.context_manager
                .jump_to_context(main_thread.get_context())
        };
        /* not return here. */
        panic!("Switching to the kernel process was failed.");
    }

    pub fn switch_to_next_thread(&mut self) {
        self._switch_to_next_thread(None)
    }

    pub fn switch_to_next_thread_without_saving_context(&mut self, current_context: &ContextData) {
        self._switch_to_next_thread(Some(current_context))
    }

    fn _switch_to_next_thread(&mut self, current_thread_context: Option<&ContextData>) {
        let interrupt_flag = InterruptManager::save_and_disable_local_irq();
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

        if running_thread.get_t_id() == next_thread.get_t_id()
            && running_thread.get_process().get_pid() == next_thread.get_process().get_pid()
        {
            /* Same Task */
            InterruptManager::restore_local_irq(interrupt_flag);
            return;
        } else {
            next_thread.set_task_status(TaskStatus::Running);
            self.running_thread = Some(next_thread as *mut _);

            if let Some(context) = current_thread_context {
                running_thread.set_context(context);
                drop(_lock);
                InterruptManager::restore_local_irq(interrupt_flag); //本来はswitch_contextですべき
                unsafe {
                    self.context_manager
                        .jump_to_context(next_thread.get_context());
                }
            } else {
                drop(_lock);
                InterruptManager::restore_local_irq(interrupt_flag); //本来はswitch_contextですべき
                unsafe {
                    self.context_manager
                        .switch_context(running_thread.get_context(), next_thread.get_context());
                }
            }
        }
    }

    /* sleep running thread and switch to next thread */
    pub fn sleep(&mut self) {
        let _lock = self.lock.lock();
        let running_thread = unsafe { &mut *self.running_thread.unwrap() };
        running_thread.set_task_status(TaskStatus::Sleeping);
        drop(_lock);
        self.switch_to_next_thread(); /* running_thread will be linked in sleep_list in this function */
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
        thread.wakeup(&mut self.run_list, &mut self.sleep_list);
        InterruptManager::restore_local_irq(flag);
        return true;
    }

    pub fn get_context_manager(&self) -> &ContextManager {
        &self.context_manager
    }

    pub fn add_kernel_thread(
        &mut self,
        entry: *const fn() -> !,
        stack_size: Option<MSize>,
        priority_level: i8,
        should_set_into_run_list: bool,
    ) -> Result<&'static mut ThreadEntry, ()> {
        if self.kernel_process.is_none() {
            return Err(());
        }
        let original_kernel_context = unsafe { &mut *self.kernel_process.unwrap() }
            .get_thread(1)
            .unwrap();

        let kernel_context = self.context_manager.create_kernel_context(
            entry as usize,
            stack_size,
            original_kernel_context.get_context(),
        );
        if kernel_context.is_err() {
            return Err(());
        }
        let _lock = self.lock.lock();
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
        let _lock = self.lock.lock();
        unsafe { &mut *self.kernel_process.unwrap() }.add_thread(thread);
        if should_set_into_run_list {
            thread.set_task_status(TaskStatus::CanRun);
            if self.run_list.get_first_entry_as_ptr().is_none() {
                thread.set_up_to_be_root_of_run_list(&mut self.run_list);
            } else {
                unsafe { self.run_list.get_last_entry_mut().unwrap() }
                    .insert_after_of_run_list(thread);
            }
        } else {
            thread.set_task_status(TaskStatus::Sleeping);
            if self.sleep_list.get_first_entry_as_ptr().is_none() {
                thread.set_up_to_be_root_of_sleep_list(&mut self.sleep_list);
            } else {
                unsafe { self.sleep_list.get_last_entry_mut().unwrap() }
                    .insert_after_of_sleep_list(thread);
            }
        }
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
