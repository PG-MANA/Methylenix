//!
//! Soft interrupt manager
//!
//! This manages delay-interrupt process.
//! The structure may be changed.

use super::TaskManager;

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::object_allocator::cache_allocator::CacheAllocator;
use crate::kernel::ptr_linked_list::{PtrLinkedList, PtrLinkedListNode};
use crate::kernel::sync::spin_lock::SpinLockFlag;
use crate::kernel::task_manager::thread_entry::ThreadEntry;

pub struct SoftInterruptManager {
    work_queue: PtrLinkedList<WorkList>,
    lock: SpinLockFlag,
    work_pool: CacheAllocator<WorkList>,
    daemon_thread: Option<*mut ThreadEntry>,
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

impl SoftInterruptManager {
    const WORK_POOL_CACHE_ENTRIES: usize = 64;

    pub fn init(&mut self, task_manager: &mut TaskManager) {
        self.lock = SpinLockFlag::new();
        let _lock = self.lock.lock();
        self.work_queue = PtrLinkedList::new();
        self.work_pool = CacheAllocator::new(0);

        if let Err(e) = self.work_pool.init(
            Self::WORK_POOL_CACHE_ENTRIES,
            &mut get_kernel_manager_cluster().memory_manager.lock().unwrap(),
        ) {
            panic!("Cannot init pool: {:?}", e);
        }

        let thread = task_manager
            .add_kernel_thread(Self::soft_interrupt_d as *const _, None, 0, false)
            .expect("Cannot add the soft interrupt daemon.");
        self.daemon_thread = Some(thread as *mut _);
    }

    pub fn add_work(&mut self, w: WorkList) {
        /* this will be called in the interrupt */

        let work = self
            .work_pool
            .alloc(Some(&get_kernel_manager_cluster().memory_manager))
            .expect("Cannot allocate work struct.");
        /* CacheAllocator will try lock memory_manager and if that was failed and pool is not enough, it will return Err. */
        *work = w;
        work.list = PtrLinkedListNode::new();

        let ptr = work as *mut _;
        work.list.set_ptr(ptr);
        let _lock = self.lock.lock();
        if let Some(last_entry) = unsafe { self.work_queue.get_last_entry_mut() } {
            let ptr = work as *mut _;
            work.list.set_ptr(ptr);
            last_entry.list.insert_after(&mut work.list);
        } else {
            self.work_queue.set_first_entry(Some(&mut work.list));
        }
        get_kernel_manager_cluster()
            .task_manager
            .wakeup_target_thread(unsafe {
                &mut *self
                    .daemon_thread
                    .expect("Cannot run the soft interrupt daemon.")
            });
    }

    fn soft_interrupt_d() -> ! {
        let manager = &mut get_kernel_manager_cluster().soft_interrupt_manager;
        loop {
            let _lock = manager.lock.lock();
            if manager.work_queue.get_first_entry_as_ptr().is_none() {
                drop(_lock);
                get_kernel_manager_cluster().task_manager.sleep();
                continue;
            }
            let work = unsafe { manager.work_queue.get_first_entry_mut().unwrap() };
            if let Some(next) = unsafe { work.list.get_next_mut() } {
                next.list.terminate_prev_entry();
                manager
                    .work_queue
                    .set_first_entry(Some(&mut next.list as *mut _));
            } else {
                manager.work_queue.set_first_entry(None);
            }
            drop(_lock);
            (work.worker_function)(work.data);
            let _lock = manager.lock.lock();
            manager.work_pool.free(work);
            drop(_lock);
        }
    }
}
