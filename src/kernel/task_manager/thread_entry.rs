/*
 * Task Manager Thread Entry
 * This entry contains some arch-depending data
 */

use super::{ProcessEntry, TaskStatus};

use crate::arch::target_arch::context::context_data::ContextData;

use crate::kernel::ptr_linked_list::{PtrLinkedList, PtrLinkedListNode};
use crate::kernel::sync::spin_lock::SpinLockFlag;

use core::ptr::NonNull;

pub struct ThreadEntry {
    lock: SpinLockFlag,
    t_list: PtrLinkedListNode<Self>, /* All thread in process */
    run_list: PtrLinkedListNode<Self>,
    sleep_list: PtrLinkedListNode<Self>,
    status: TaskStatus,
    thread_id: usize,
    process: NonNull<ProcessEntry>,
    context_data: ContextData,
    privilege_level: u8,
    priority_level: i8,
}

impl ThreadEntry {
    pub const THREAD_ENTRY_ALIGN_ORDER: usize = 0;

    pub fn init(
        &mut self,
        process: *mut ProcessEntry,
        privilege_level: u8,
        priority_level: i8,
        context_data: ContextData,
    ) {
        self.lock = SpinLockFlag::new();
        let _lock = self.lock.lock();
        self.t_list = PtrLinkedListNode::new();
        self.run_list = PtrLinkedListNode::new();
        self.sleep_list = PtrLinkedListNode::new();
        self.status = TaskStatus::New;
        self.thread_id = 0;
        self.process = NonNull::new(process).unwrap();
        self.context_data = context_data;
        self.privilege_level = privilege_level;
        self.priority_level = priority_level;
    }

    pub fn set_up_to_be_root_of_p_list(&mut self, list_head: &mut PtrLinkedList<Self>) {
        let _lock = self.lock.lock();
        let ptr = self as *mut _;
        self.t_list.set_ptr(ptr);
        self.t_list.terminate_prev_entry();
        list_head.set_first_entry(Some(&mut self.t_list));
    }

    pub fn insert_after_of_p_list(&mut self, entry: &mut Self) {
        let _lock = self.lock.lock();
        if entry.t_list.is_invalid_ptr() {
            let ptr = entry as *mut Self;
            entry.t_list.set_ptr(ptr);
        }
        let ptr = self as *mut _;
        self.t_list.set_ptr(ptr);
        self.t_list.insert_after(&mut entry.t_list);
    }

    pub fn set_up_to_be_root_of_run_list(&mut self, list_head: &mut PtrLinkedList<Self>) {
        let _lock = self.lock.lock();
        let ptr = self as *mut _;
        self.run_list.set_ptr(ptr);
        self.run_list.terminate_prev_entry();
        list_head.set_first_entry(Some(&mut self.run_list));
    }

    pub fn set_up_to_be_root_of_sleep_list(&mut self, list_head: &mut PtrLinkedList<Self>) {
        let _lock = self.lock.lock();
        let ptr = self as *mut _;
        self.sleep_list.set_ptr(ptr);
        self.sleep_list.terminate_prev_entry();
        list_head.set_first_entry(Some(&mut self.sleep_list));
    }

    pub fn insert_after_of_run_list(&mut self, entry: &mut Self) {
        let _lock = self.lock.lock();
        if entry.run_list.is_invalid_ptr() {
            let ptr = entry as *mut Self;
            entry.run_list.set_ptr(ptr);
        }
        let ptr = self as *mut _;
        self.run_list.set_ptr(ptr);
        self.run_list.insert_after(&mut entry.run_list);
    }

    pub fn insert_after_of_sleep_list(&mut self, entry: &mut Self) {
        let _lock = self.lock.lock();
        if entry.sleep_list.is_invalid_ptr() {
            let ptr = entry as *mut Self;
            entry.sleep_list.set_ptr(ptr);
        }
        let ptr = self as *mut _;
        self.sleep_list.set_ptr(ptr);
        self.sleep_list.insert_after(&mut entry.run_list);
    }

    pub fn set_process(&mut self, process: *mut ProcessEntry) {
        let _lock = self.lock.lock();
        self.process = NonNull::new(process).unwrap();
    }

    pub fn get_process(&self) -> &ProcessEntry {
        unsafe { self.process.as_ref() }
    }

    pub const fn get_task_status(&self) -> TaskStatus {
        self.status
    }

    pub const fn get_t_id(&self) -> usize {
        self.thread_id
    }

    pub fn set_t_id(&mut self, t_id: usize) {
        let _lock = self.lock.lock();
        self.thread_id = t_id;
    }

    pub fn set_task_status(&mut self, status: TaskStatus) {
        let _lock = self.lock.lock();
        self._set_task_status(status);
    }

    const fn _set_task_status(&mut self, status: TaskStatus) {
        self.status = status;
    }

    pub fn get_context(&mut self) -> &mut ContextData {
        &mut self.context_data
    }

    pub fn set_context(&mut self, context: &ContextData) {
        unsafe {
            core::ptr::copy(context as *const _, &mut self.context_data as *mut _, 1);
        }
    }

    pub fn get_next_from_run_list_mut(&mut self) -> Option<&'static mut Self> {
        unsafe { self.run_list.get_next_mut() }
    }

    pub fn insert_self_to_sleep_queue(
        &mut self,
        sleep_queue_head: &mut PtrLinkedList<Self>,
        run_queue_head: &mut PtrLinkedList<Self>,
    ) {
        let _lock = self.lock.lock();
        assert_eq!(self.status, TaskStatus::Sleeping);
        let old_first_entry = unsafe { sleep_queue_head.get_first_entry_mut() };
        if old_first_entry.is_none() {
            self.sleep_list.terminate_prev_entry();
            let ptr = self as *mut _;
            self.sleep_list.set_ptr(ptr);
            sleep_queue_head.set_first_entry(Some(&mut self.sleep_list as *mut _));
        } else {
            self.sleep_list.terminate_prev_entry();
            let ptr = self as *mut _;
            self.sleep_list.set_ptr(ptr);
            old_first_entry
                .unwrap()
                .sleep_list
                .insert_before(&mut self.sleep_list);
            sleep_queue_head.set_first_entry(Some(&mut self.sleep_list as *mut _));
        }
        self.run_list.remove_from_list(run_queue_head);
    }

    pub fn is_root_of_run_list(&self) -> bool {
        self.run_list.get_prev_as_ptr().is_none()
    }

    pub fn wakeup(
        &mut self,
        run_queue_head: &mut PtrLinkedList<Self>,
        sleep_queue_head: &mut PtrLinkedList<Self>,
    ) {
        let _lock = self.lock.lock();
        assert_eq!(self.status, TaskStatus::Sleeping);
        let ptr = self as *mut _;
        self.run_list.set_ptr(ptr);

        let old_last_entry = unsafe { run_queue_head.get_last_entry_mut() };
        if old_last_entry.is_none() {
            self.run_list.terminate_prev_entry();
            run_queue_head.set_first_entry(Some(&mut self.run_list as *mut _));
        } else {
            self.run_list.terminate_prev_entry();
            old_last_entry
                .unwrap()
                .run_list
                .insert_after(&mut self.run_list);
        }
        self._set_task_status(TaskStatus::CanRun);
        self.sleep_list.remove_from_list(sleep_queue_head);
    }
}
