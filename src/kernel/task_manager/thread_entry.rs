//!
//! Task Manager Thread Entry
//!
//! This entry contains some arch-depending data

use super::{scheduling_class::SchedulingClass, ProcessEntry, TaskStatus};

use crate::arch::target_arch::context::context_data::ContextData;

use crate::kernel::collections::ptr_linked_list::PtrLinkedListNode;
use crate::kernel::sync::spin_lock::SpinLockFlag;

use core::ptr::NonNull;

/// ThreadEntry's method does not lock `Self::lock`, **the caller must lock it**.
pub struct ThreadEntry {
    pub(super) t_list: PtrLinkedListNode<Self>, /* All thread in process */
    pub(super) run_list: PtrLinkedListNode<Self>,
    pub(super) sleep_list: PtrLinkedListNode<Self>,
    pub(super) lock: SpinLockFlag,
    pub(super) time_slice: u64,

    status: TaskStatus,
    thread_id: usize,
    process: NonNull<ProcessEntry>,
    context_data: ContextData,
    privilege_level: u8,
    priority_level: u8,
    scheduling_class: SchedulingClass,
}

impl ThreadEntry {
    pub const THREAD_ENTRY_ALIGN: usize = 0;

    pub fn init(
        &mut self,
        process: *mut ProcessEntry,
        privilege_level: u8,
        priority_level: u8,
        scheduling_class: SchedulingClass,
        context_data: ContextData,
    ) {
        self.lock = SpinLockFlag::new();
        let _lock = self.lock.lock();
        self.t_list = PtrLinkedListNode::new();
        self.run_list = PtrLinkedListNode::new();
        self.sleep_list = PtrLinkedListNode::new();
        self.time_slice = 0;
        self.status = TaskStatus::New;
        self.thread_id = 0;
        self.process = NonNull::new(process).unwrap();
        self.context_data = context_data;
        self.privilege_level = privilege_level;
        self.priority_level = priority_level;
        self.scheduling_class = scheduling_class;
    }

    pub fn fork_data(&mut self, original_thread: &Self, context_data: ContextData) {
        assert!(original_thread.lock.is_locked());
        self.lock = SpinLockFlag::new();
        let _lock = self.lock.lock();
        self.t_list = PtrLinkedListNode::new();
        self.run_list = PtrLinkedListNode::new();
        self.sleep_list = PtrLinkedListNode::new();
        self.time_slice = 0;
        self.status = TaskStatus::New;
        self.thread_id = 0;
        self.process = original_thread.process;
        self.privilege_level = original_thread.privilege_level;
        self.priority_level = original_thread.priority_level;
        self.scheduling_class = original_thread.scheduling_class;
        self.context_data = context_data;
    }

    pub fn set_process(&mut self, process: *mut ProcessEntry) {
        self.process = NonNull::new(process).unwrap();
    }

    pub fn get_process(&self) -> &'static ProcessEntry {
        unsafe { &*self.process.as_ptr() }
    }

    pub fn get_process_mut(&mut self) -> &'static mut ProcessEntry {
        unsafe { &mut *self.process.as_ptr() }
    }

    pub const fn get_task_status(&self) -> TaskStatus {
        self.status
    }

    pub const fn get_t_id(&self) -> usize {
        self.thread_id
    }

    pub const fn get_priority_level(&self) -> u8 {
        self.priority_level
    }

    pub const fn get_privilege_level(&self) -> u8 {
        self.privilege_level
    }

    pub fn set_priority_level(&mut self, p: u8) {
        self.priority_level = p;
    }

    pub fn set_t_id(&mut self, t_id: usize) {
        self.thread_id = t_id;
    }

    pub fn set_task_status(&mut self, status: TaskStatus) {
        self.status = status;
    }

    pub fn get_context(&mut self) -> &mut ContextData {
        &mut self.context_data
    }

    pub fn set_context(&mut self, context: &ContextData) {
        self.context_data = context.clone();
    }

    pub fn copy_data(&self) -> Self {
        Self {
            t_list: PtrLinkedListNode::new(),
            run_list: PtrLinkedListNode::new(),
            sleep_list: PtrLinkedListNode::new(),
            time_slice: 0,
            lock: SpinLockFlag::new(),
            status: self.status,
            thread_id: self.thread_id,
            process: self.process,
            context_data: self.context_data.clone(),
            privilege_level: self.privilege_level,
            priority_level: self.priority_level,
            scheduling_class: self.scheduling_class,
        }
    }

    pub fn set_time_slice(&mut self, number_of_threads: usize, timer_interval: u64) {
        self.time_slice = self.scheduling_class.calculate_time_slice(
            self.priority_level,
            number_of_threads,
            timer_interval,
        );
    }
}
