//use(Arch依存)
use arch::target_arch::paging::PageManager;

//use(Arch非依存)
use kernel::memory_manager::MemoryManager;

//use(Core)
use core::mem;

const ENTRY_SIZE: usize = mem::size_of::<TaskEntry>();

#[derive(Copy, Clone, PartialEq)]
pub enum TaskStatus {
    Running,
    Waiting,
    Stopping,
    Exiting,
    Deleting,
}

pub struct TaskMemoryInfo {
    page_manager: PageManager,
    kernel_stack: usize,
    /*未計画*/
}

pub struct TaskEntry {
    status: TaskStatus,
    pid: usize,
    privilege_level: u8,
    memory_info: TaskMemoryInfo,
    previous: usize,
    next: usize,
    enabled: bool,
    /*識別用*/
}

pub struct TaskManager {
    entry: usize,
    entry_pool: usize,
    entry_pool_size: usize,
    running_task_pid: usize,
}

impl TaskManager {
    pub const fn new() -> TaskManager {
        TaskManager {
            entry: 0,
            entry_pool: 0,
            entry_pool_size: 0,
            running_task_pid: 0,
        }
    }

    pub fn set_entry_pool(&mut self, free_address: usize, free_address_size: usize) {
        self.entry_pool = free_address;
        self.entry_pool_size = free_address_size;
        for i in 0..(free_address_size / ENTRY_SIZE) {
            unsafe { (*((free_address + i * ENTRY_SIZE) as *mut TaskEntry)).set_disabled() }
        }
        let dummy_entry = unsafe { &mut *(free_address as *mut TaskEntry) };
        *dummy_entry = TaskEntry::new(0, TaskMemoryInfo { page_manager: PageManager::new_static(), kernel_stack: 0 }, 3, dummy_entry, dummy_entry);
        self.entry = free_address;
    }

    fn create_task_entry(&mut self) -> Option<&'static mut TaskEntry> {
        /*TODO: CLIなどのMutex処理*/
        for i in 0..(self.entry_pool_size / ENTRY_SIZE) {
            let entry = unsafe { &mut *((self.entry_pool + i * ENTRY_SIZE) as *mut TaskEntry) };
            if !entry.is_enabled() {
                entry.set_enabled();
                entry.set_pid(i);
                return Some(entry);
            }
        }
        None
    }

    fn get_target_task_entry(&mut /*mutを返すのでmutじゃないとまずいかなぁ*/ self, pid: usize) -> &'static mut TaskEntry {
        unsafe { (&mut *((self.entry_pool + pid * ENTRY_SIZE) as *mut TaskEntry)) }
    }

    pub fn create_new_task(&mut self, memory_manager: &mut MemoryManager) -> Option<usize>/*pid*/ {
        if let Some(task) = self.create_task_entry() {
            task.set_status(TaskStatus::Stopping);
            task.set_privilege_level(3);
            task.memory_info.page_manager.init(memory_manager,None);//TODO: 要調節
            if let Some(stack) = memory_manager.alloc_page(true) {
                task.memory_info.kernel_stack = stack;
                unsafe { &mut *((&mut *(self.entry as *mut TaskEntry)).previous as *mut TaskEntry) }.chain_after_me(task);
                return Some(task.get_pid());
            } else {
                task.delete();
            }
        }
        None
    }

    pub fn assign_memory(&mut self, memory_manager: &mut MemoryManager, pid: usize, physical_address: usize, linear_address: usize, is_code: bool, is_writable: bool) {
        let task = self.get_target_task_entry(pid);
        let is_user_accessible = if task.get_privilege_level() == 3 { true } else { false };
        task.memory_info.page_manager.associate_address(memory_manager,None/*TODO:調節*/, physical_address, linear_address, is_code, is_writable, is_user_accessible);
    }

    pub fn add_task(&mut self, target_task: TaskEntry) -> bool {
        /*暫定的実装*/
        if let Some(task) = self.create_task_entry() {
            task.memory_info = target_task.memory_info;
            task.privilege_level = target_task.privilege_level;
            task.status = target_task.status;
            unsafe { &mut *((&mut *(self.entry as *mut TaskEntry)).previous as *mut TaskEntry) }.chain_after_me(task);
            task.chain_after_me(unsafe { &mut *(self.entry as *mut TaskEntry) });
            true
        } else {
            false
        }
    }

    pub fn get_running_task_page_manager(&mut self) -> &'static mut PageManager {
        /*TODO:同時アクセス処理*/
        if self.running_task_pid == 0 {
            panic!("TaskManager is broken.");
        }
        &mut self.get_target_task_entry(self.running_task_pid).memory_info.page_manager
    }

    pub fn switch_next_task(&mut self, should_set_paging: bool) {
        let mut next_task_pid = self.running_task_pid;
        loop {
            let entry = unsafe { &mut *(self.get_target_task_entry(next_task_pid).next as *mut TaskEntry) };
            next_task_pid = entry.get_pid();
            if next_task_pid == self.running_task_pid {
                return;
            }
            if entry.is_enabled() && entry.get_status() == TaskStatus::Running {
                break;
            }
        }
        self.running_task_pid = next_task_pid;
        if should_set_paging {
            self.get_running_task_page_manager().reset_paging();
        }
    }
}

impl TaskEntry {
    pub fn new(process_id: usize, mem_info: TaskMemoryInfo, privilege: u8, previous: &TaskEntry, next: &TaskEntry) -> TaskEntry {
        TaskEntry {
            status: TaskStatus::Stopping,
            pid: process_id,
            privilege_level: privilege,
            memory_info: mem_info,
            previous: previous as *const TaskEntry as usize,
            next: next as *const TaskEntry as usize,
            enabled: true,
        }
    }

    pub const fn new_static() -> TaskEntry {
        TaskEntry {
            status: TaskStatus::Stopping,
            pid: 0,
            privilege_level: 3,
            memory_info: TaskMemoryInfo {
                kernel_stack: 0,
                page_manager: PageManager::new_static(),
            },
            previous: 0,
            next: 0,
            enabled: false,
        }
    }

    pub fn chain_after_me(&mut self, entry: &mut TaskEntry) {
        self.next = entry as *mut Self as usize;
        unsafe { (&mut *(entry as *mut Self)).previous = self as *mut Self as usize; }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_disabled(&mut self) {
        self.enabled = false;
    }

    pub fn set_enabled(&mut self) {
        self.enabled = true;
    }

    pub fn get_pid(&self) -> usize {
        self.pid
    }

    pub fn set_pid(&mut self, i: usize) {
        self.pid = i;
    }

    pub fn get_privilege_level(&self) -> u8 {
        self.privilege_level
    }

    pub fn set_privilege_level(&mut self, level: u8) {
        self.privilege_level = level;
    }

    pub fn get_status(&self) -> TaskStatus {
        self.status
    }

    pub fn set_status(&mut self, s: TaskStatus) {
        self.status = s;
    }

    pub fn set_page_manager(&mut self, page_manager: PageManager) {
        self.memory_info.page_manager = page_manager;
    }

    pub fn set_kernel_stack(&mut self, kernel_stack_address: usize) {
        self.memory_info.kernel_stack = kernel_stack_address;
    }

    pub fn delete(&mut self) {
        self.set_disabled();
        unsafe { (&mut *(self.previous as *mut Self)).next = self.next as *mut Self as usize; }
    }
}