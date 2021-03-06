//!
//! Work Queue
//!
//! This module manages delay-interrupt process.
//! The structure may be changed.

use super::{TaskManager, TaskStatus, ThreadEntry};

use crate::arch::target_arch::interrupt::InterruptManager;

use crate::kernel::collections::ptr_linked_list::{PtrLinkedList, PtrLinkedListNode};
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::object_allocator::cache_allocator::CacheAllocator;
use crate::kernel::sync::spin_lock::SpinLockFlag;

pub struct WorkQueue {
    work_queue: PtrLinkedList<WorkList>,
    lock: SpinLockFlag,
    work_pool: CacheAllocator<WorkList>,
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
        self.lock = SpinLockFlag::new();
        let _lock = self.lock.lock();
        self.work_queue = PtrLinkedList::new();
        self.work_pool = CacheAllocator::new(0);

        self.work_pool
            .init(
                Self::WORK_POOL_CACHE_ENTRIES,
                &mut get_kernel_manager_cluster().memory_manager.lock().unwrap(),
            )
            .expect("Cannot init memory pool for WorkList.");

        let thread = task_manager
            .create_kernel_thread_for_init(Self::work_queue_thread, None, Self::DEFAULT_PRIORITY)
            .expect("Cannot add the soft interrupt daemon.");
        self.daemon_thread = thread as *mut _;
    }

    pub fn add_work(&mut self, w: WorkList) {
        /* This will be called in the interrupt */
        let irq_data = InterruptManager::save_and_disable_local_irq();
        let _lock = self.lock.lock();

        let work = self
            .work_pool
            .alloc(Some(&get_kernel_manager_cluster().memory_manager))
            .expect("Cannot allocate work struct.");
        /* CacheAllocator will try lock memory_manager and if that was failed and pool is not enough, it will return Err. */
        *work = w;
        work.list = PtrLinkedListNode::new();

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
        drop(_lock);
        InterruptManager::restore_local_irq(irq_data);
    }

    fn work_queue_thread() -> ! {
        let manager = &mut get_cpu_manager_cluster().work_queue;
        loop {
            let interrupt_flag = InterruptManager::save_and_disable_local_irq();
            let _lock = manager.lock.lock();
            if manager.work_queue.is_empty() {
                assert!(!unsafe { &mut *manager.daemon_thread }.lock.is_locked());
                drop(_lock);
                if let Err(e) = get_cpu_manager_cluster()
                    .run_queue
                    .sleep_current_thread(Some(interrupt_flag), TaskStatus::Interruptible)
                {
                    pr_err!("Cannot sleep work queue thread. Error: {:?}", e);
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
            InterruptManager::restore_local_irq(interrupt_flag);
            /* Execute the work function */
            work_function(work_data);
        }
    }
}
