//!
//! Work Queue
//!
//! This module manages delay-interrupt process.
//! The structure may be changed.
//! Work Queue will be in per-cpu structure, therefore this uses disabling interrupt as a lock.

use super::{TaskError, TaskManager, TaskStatus, ThreadEntry};

use crate::arch::target_arch::interrupt::InterruptManager;

use crate::kernel::collections::ptr_linked_list::{PtrLinkedList, PtrLinkedListNode};
use crate::kernel::manager_cluster::get_cpu_manager_cluster;
use crate::kernel::memory_manager::slab_allocator::LocalSlabAllocator;
use crate::kernel::sync::spin_lock::IrqSaveSpinLockFlag;

use core::mem::offset_of;

pub struct WorkQueue {
    global_lock: IrqSaveSpinLockFlag,
    work_queue: PtrLinkedList<WorkList>,
    work_pool: LocalSlabAllocator<WorkList>,
    daemon_thread: *mut ThreadEntry,
}

pub struct WorkList {
    list: PtrLinkedListNode<Self>,
    worker_function: fn(usize),
    data: usize,
}

impl WorkList {
    pub const fn new(worker_function: fn(usize), data: usize) -> Self {
        Self {
            worker_function,
            data,
            list: PtrLinkedListNode::new(),
        }
    }
}

impl WorkQueue {
    const DEFAULT_PRIORITY: u8 = 10;

    pub fn init_work_queue(&mut self, task_manager: &mut TaskManager) {
        self.work_queue = PtrLinkedList::new();
        self.work_pool = LocalSlabAllocator::new(0);

        self.work_pool
            .init()
            .expect("Failed to init memory pool for WorkList");

        let thread = task_manager
            .create_kernel_thread_with_argument(
                Self::work_queue_global_thread,
                None,
                Self::DEFAULT_PRIORITY,
                self as *mut _ as usize,
            )
            .expect("Failed to add the soft interrupt daemon");
        self.daemon_thread = thread as *mut _;
    }

    pub fn init_cpu_work_queue(&mut self, task_manager: &mut TaskManager) {
        self.work_queue = PtrLinkedList::new();
        self.work_pool = LocalSlabAllocator::new(0);

        self.work_pool
            .init()
            .expect("Failed to init memory pool for WorkList");

        let thread = task_manager
            .create_kernel_thread_for_init(
                Self::work_queue_local_thread,
                None,
                Self::DEFAULT_PRIORITY,
                TaskManager::FLAG_LOCAL_THREAD,
            )
            .expect("Failed to add the soft interrupt daemon");
        self.daemon_thread = thread as *mut _;
    }

    pub fn add_work(&mut self, w: WorkList) -> Result<(), TaskError> {
        /* This will be called in the interrupt handler */
        let irq = InterruptManager::save_and_disable_local_irq();

        let work = match self.work_pool.alloc() {
            Ok(work) => {
                *work = w;
                work.list = PtrLinkedListNode::new();
                work
            }
            Err(err) => {
                pr_err!("Failed to allocate a WorkList: {:?}", err);
                return Err(TaskError::MemoryError(err));
            }
        };

        self.work_queue.insert_tail(&mut work.list);
        let worker_thread = unsafe { &mut *self.daemon_thread };
        let _worker_thread_lock = worker_thread.lock.lock();
        if worker_thread.get_task_status() != TaskStatus::Running {
            let run_queue = &mut get_cpu_manager_cluster().run_queue;
            if let Err(err) = run_queue.add_thread(worker_thread) {
                pr_err!("Failed to add worker_thread into RunQueue: {:?}", err);
                drop(_worker_thread_lock);
                InterruptManager::restore_local_irq(irq);
                return Err(err);
            }
        }
        drop(_worker_thread_lock);
        InterruptManager::restore_local_irq(irq);
        return Ok(());
    }

    fn work_queue_local_thread() -> ! {
        let manager = &mut get_cpu_manager_cluster().work_queue;
        loop {
            let irq = InterruptManager::save_and_disable_local_irq();
            if manager.work_queue.is_empty() {
                assert!(!unsafe { &mut *manager.daemon_thread }.lock.is_locked());
                if let Err(e) = get_cpu_manager_cluster()
                    .run_queue
                    .sleep_current_thread(Some(irq), TaskStatus::Interruptible)
                {
                    pr_err!("Failed to sleep work queue thread: {:?}", e);
                }
                /* Woke up */
                continue;
            }
            let work = unsafe {
                manager
                    .work_queue
                    .take_first_entry(offset_of!(WorkList, list))
                    .unwrap()
            };
            let work_function = work.worker_function;
            let work_data = work.data;
            manager.work_pool.free(work);
            InterruptManager::restore_local_irq(irq);
            /* Execute the work function */
            work_function(work_data);
        }
    }

    fn work_queue_global_thread(manager_address: usize) -> ! {
        let manager = unsafe { &mut *(manager_address as *mut Self) };
        loop {
            let _lock = manager.global_lock.lock();
            if manager.work_queue.is_empty() {
                assert!(!unsafe { &mut *manager.daemon_thread }.lock.is_locked());
                drop(_lock);
                if let Err(e) = get_cpu_manager_cluster()
                    .run_queue
                    .sleep_current_thread(None, TaskStatus::Interruptible)
                {
                    pr_err!("Failed to sleep work queue thread: {:?}", e);
                }
                /* Woke up */
                continue;
            }
            let work = unsafe {
                manager
                    .work_queue
                    .take_first_entry(offset_of!(WorkList, list))
                    .unwrap()
            };
            let work_function = work.worker_function;
            let work_data = work.data;
            manager.work_pool.free(work);
            drop(_lock);
            /* Execute the work function */
            work_function(work_data);
        }
    }
}
