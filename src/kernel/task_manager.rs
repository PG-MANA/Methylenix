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
use arch::target_arch::device::cpu::halt;

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

pub enum TaskSignal {
    Normal,
    Stop,
    Kill,
}

pub enum TaskStatus {
    New,
    Running,
    Zombie,
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

    pub fn init(&mut self, context_manager: ContextManager) {
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

        self.context_manager = context_manager;
    }

    pub fn create_init_process(&mut self, entry_point: usize) {
        use core::i8;
        let process_entry = self.process_entry_pool.alloc().unwrap();
        let thread_entry = self.thread_entry_pool.alloc().unwrap();
        let idle_thread_entry = self.thread_entry_pool.alloc().unwrap();

        let mut kernel_memory_alloc_manager = get_kernel_manager_cluster()
            .kernel_memory_alloc_manager
            .lock()
            .unwrap();
        let memory_manager = &get_kernel_manager_cluster().memory_manager;
        let stack_for_init = kernel_memory_alloc_manager
            .vmalloc(
                ContextManager::DEFAULT_STACK_SIZE_OF_SYSTEM,
                ContextManager::DEFAULT_STACK_SIZE_OF_USER,
                memory_manager,
            )
            .unwrap();
        let stack_for_idle = kernel_memory_alloc_manager
            .vmalloc(
                ContextManager::DEFAULT_STACK_SIZE_OF_SYSTEM,
                ContextManager::DEFAULT_STACK_SIZE_OF_USER,
                memory_manager,
            )
            .unwrap();
        drop(kernel_memory_alloc_manager);

        let context_data_for_init = self
            .context_manager
            .create_system_context(entry_point, stack_for_init);
        let context_data_for_idle = self
            .context_manager
            .create_system_context(idle as *const fn() as usize, stack_for_idle);

        thread_entry.init(1, process_entry, 0, 0, context_data_for_init);
        idle_thread_entry.init(2, process_entry, 0, i8::MIN, context_data_for_idle);

        process_entry.init(1, thread_entry, 0, 0);
        process_entry.add_thread(idle_thread_entry);
    }
}

fn idle() -> ! {
    loop {
        unsafe {
            halt();
        }
    }
}
