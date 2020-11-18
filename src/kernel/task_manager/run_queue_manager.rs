//!
//! Task Run Queue Manager
//!
//! This module manages per-cpu run queue.

use super::thread_entry::ThreadEntry;
use super::TaskStatus;

use crate::arch::target_arch::context::context_data::ContextData;
use crate::arch::target_arch::interrupt::InterruptManager;

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::ptr_linked_list::PtrLinkedList;
use crate::kernel::sync::spin_lock::SpinLockFlag;

pub struct RunQueueManager {
    lock: SpinLockFlag,
    run_list: PtrLinkedList<ThreadEntry>,
    running_thread: Option<*mut ThreadEntry>,
}

impl RunQueueManager {
    pub const fn new() -> Self {
        Self {
            lock: SpinLockFlag::new(),
            run_list: PtrLinkedList::new(),
            running_thread: None,
        }
    }

    pub fn init(&mut self, idle_function: *const fn() -> !) {
        let _lock = self.lock.lock();
        let idle_thread = get_kernel_manager_cluster()
            .task_manager
            .create_kernel_thread(idle_function, None, i8::MIN)
            .expect("Cannot create idle thread.");
        idle_thread.set_up_to_be_root_of_run_list(&mut self.run_list);
    }

    pub fn start(&mut self) -> ! {
        let _lock = self.lock.lock();
        let thread = unsafe { self.run_list.get_first_entry_mut().unwrap() };

        thread.set_task_status(TaskStatus::Running);
        self.running_thread = Some(thread);
        drop(_lock);
        unsafe {
            get_kernel_manager_cluster()
                .task_manager
                .get_context_manager()
                .jump_to_context(thread.get_context())
        };
        /* not return here. */
        panic!("Switching to the kernel process was failed.");
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

    pub fn add_thread(&mut self, thread: &mut ThreadEntry) {
        let _lock = self.lock.lock();
        if let Some(e) = unsafe { self.run_list.get_last_entry_mut() } {
            e.insert_after_of_run_list(thread);
        } else {
            thread.set_up_to_be_root_of_run_list(&mut self.run_list);
        }
        thread.set_task_status(TaskStatus::CanRun);
    }

    pub fn wakeup(
        &mut self,
        thread: &mut ThreadEntry,
        sleep_list: &mut PtrLinkedList<ThreadEntry>,
    ) {
        let _lock = self.lock.lock();
        thread.wakeup(&mut self.run_list, sleep_list);
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
            get_kernel_manager_cluster()
                .task_manager
                .insert_into_sleep_list(running_thread, &mut self.run_list);
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
                    get_kernel_manager_cluster()
                        .task_manager
                        .get_context_manager()
                        .jump_to_context(next_thread.get_context());
                }
            } else {
                drop(_lock);
                InterruptManager::restore_local_irq(interrupt_flag); //本来はswitch_contextですべき
                unsafe {
                    get_kernel_manager_cluster()
                        .task_manager
                        .get_context_manager()
                        .switch_context(running_thread.get_context(), next_thread.get_context());
                }
            }
        }
    }
}
