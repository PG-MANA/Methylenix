/*
 * Task Manager
 * This manager is the frontend of task management system.
 * Task management system has two struct, arch-independent and depend on arch.
 */

mod process_entry;
mod thread_entry;

use self::process_entry::ProcessEntry;
use self::thread_entry::ThreadEntry;

use arch::target_arch::context::ContextManager;

use kernel::manager_cluster::get_kernel_manager_cluster;
use kernel::memory_manager::pool_allocator::PoolAllocator;
use kernel::ptr_linked_list::PtrLinkedList;

use core::mem;

pub struct TaskManager {
    run_list: PtrLinkedList<ThreadEntry>,
    sleep_list: PtrLinkedList<ThreadEntry>,
    running_thread: Option<*mut ThreadEntry>,
    context_manager: ContextManager,
    process_entry_pool: PoolAllocator<ProcessEntry>,
    thread_entry_pool: PoolAllocator<ThreadEntry>,
}

impl TaskManager {
    const NUM_OF_INITIAL_THREAD_ENTRIES: usize = 6;
    const NUM_OF_INITIAL_PROCESS_ENTRIES: usize = 6;
    pub const fn new() -> Self {
        Self {
            run_list: PtrLinkedList::new(),
            sleep_list: PtrLinkedList::new(),
            running_thread: None,
            context_manager: ContextManager::new(),
            process_entry_pool: PoolAllocator::new(),
            thread_entry_pool: PoolAllocator::new(),
        }
    }

    pub fn init(&mut self) {
        let memory_manager = &get_kernel_manager_cluster().memory_manager;
        let mut kernel_memory_allocator = get_kernel_manager_cluster()
            .kernel_memory_alloc_manager
            .lock()
            .unwrap();

        for _i in 0..Self::NUM_OF_INITIAL_PROCESS_ENTRIES {
            let address = kernel_memory_allocator
                .vmalloc(
                    mem::size_of::<ProcessEntry>(),
                    ProcessEntry::PROCESS_ENTRY_ALIGN_ORDER,
                    memory_manager,
                )
                .unwrap();
            self.process_entry_pool
                .free_ptr(address as *mut ProcessEntry);
        }

        for _i in 0..Self::NUM_OF_INITIAL_THREAD_ENTRIES {
            let address = kernel_memory_allocator
                .vmalloc(
                    mem::size_of::<ThreadEntry>(),
                    ThreadEntry::THREAD_ENTRY_ALIGN_ORDER,
                    memory_manager,
                )
                .unwrap();
            self.thread_entry_pool.free_ptr(address as *mut ThreadEntry);
        }

        self.context_manager.init();
    }

    pub fn create_new_process(&mut self) -> Option<()> {
        None
    }
}
