//!
//! Work Queue
//!
//! This module manages delay-interrupt process.
//! The structure may be changed.
//! Work Queue will be in per-cpu structure, therefore this uses disabling interrupt as a lock.

use super::{TaskManager, TaskStatus, ThreadEntry};

use crate::arch::target_arch::interrupt::InterruptManager;

use crate::kernel::collections::ptr_linked_list::{PtrLinkedList, PtrLinkedListNode};
use crate::kernel::manager_cluster::get_cpu_manager_cluster;
use crate::kernel::memory_manager::slab_allocator::LocalSlabAllocator;

pub struct WorkQueue {
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
    const WORK_POOL_CACHE_ENTRIES: usize = 64;
    const DEFAULT_PRIORITY: u8 = 10;

    pub fn init(&mut self, task_manager: &mut TaskManager) {
        let irq = InterruptManager::save_and_disable_local_irq();
        self.work_queue = PtrLinkedList::new();
        self.work_pool = LocalSlabAllocator::new(0);

        self.work_pool
            .init()
            .expect("Failed to init memory pool for WorkList");

        let thread = task_manager
            .create_kernel_thread_for_init(Self::work_queue_thread, None, Self::DEFAULT_PRIORITY)
            .expect("Cannot add the soft interrupt daemon.");
        self.daemon_thread = thread as *mut _;
        InterruptManager::restore_local_irq(irq);
    }

    pub fn add_work(&mut self, w: WorkList) -> Result<(), ()> {
        /* This will be called in the interrupt */
        let irq = InterruptManager::save_and_disable_local_irq();

        let work = match self.work_pool.alloc() {
            Ok(work) => {
                *work = w;
                work.list = PtrLinkedListNode::new();
                work
            }
            Err(e) => {
                pr_err!("Failed to allocate work list: {:?}", e);
                return Err(());
            }
        };

        self.work_queue.insert_tail(&mut work.list);

        let worker_thread = unsafe { &mut *self.daemon_thread };
        let _worker_thread_lock = worker_thread.lock.lock();
        if worker_thread.get_task_status() != TaskStatus::Running {
            let run_queue = &mut get_cpu_manager_cluster().run_queue;
            if let Err(e) = run_queue.add_thread(worker_thread) {
                pr_err!(
                    "Cannot add worker_thread to RunQueueManager. Error: {:?}",
                    e
                );
            }
        }
        drop(_worker_thread_lock);
        InterruptManager::restore_local_irq(irq);
        return Ok(());
    }

    fn work_queue_thread() -> ! {
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
}
