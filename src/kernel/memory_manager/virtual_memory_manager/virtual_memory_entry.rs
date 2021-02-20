//!
//! Virtual Memory Entry Chain
//!

use super::virtual_memory_object::VirtualMemoryObject;

use crate::kernel::memory_manager::data_type::{Address, MOffset, MSize, VAddress};
use crate::kernel::memory_manager::{MemoryOptionFlags, MemoryPermissionFlags};

use crate::kernel::ptr_linked_list::{PtrLinkedList, PtrLinkedListNode};
use crate::kernel::sync::spin_lock::SpinLockFlag;

#[allow(dead_code)]
pub struct VirtualMemoryEntry {
    lock: SpinLockFlag,
    list: PtrLinkedListNode<Self>,
    start_address: VAddress,
    end_address: VAddress,
    is_shared: bool,
    should_cow: bool,
    permission_flags: MemoryPermissionFlags,
    option_flags: MemoryOptionFlags,
    object: VirtualMemoryObject,
    offset: MOffset,
}
// ADD: thread chain

impl VirtualMemoryEntry {
    pub const ENTRY_SIZE: usize = core::mem::size_of::<Self>();

    pub const fn new(
        vm_start_address: VAddress,
        vm_end_address: VAddress,
        permission: MemoryPermissionFlags,
        option: MemoryOptionFlags,
    ) -> Self {
        Self {
            lock: SpinLockFlag::new(),
            list: PtrLinkedListNode::new(),
            start_address: vm_start_address,
            end_address: vm_end_address,
            object: VirtualMemoryObject::new(),
            is_shared: false,
            should_cow: false,
            permission_flags: permission,
            option_flags: option,
            offset: MOffset::new(0),
        }
    }

    pub const fn get_vm_start_address(&self) -> VAddress {
        self.start_address
    }

    pub const fn get_vm_end_address(&self) -> VAddress {
        self.end_address
    }

    pub fn set_vm_end_address(&mut self, new_end_address: VAddress) {
        let _lock = self.lock.lock();
        if let Some(next_entry) = self.get_next_entry() {
            assert!(next_entry.get_vm_start_address() > new_end_address);
        }
        self.end_address = new_end_address;
    }

    pub fn get_size(&self) -> MSize {
        MSize::from_address(self.get_vm_start_address(), self.get_vm_end_address())
    }

    pub const fn get_memory_offset(&self) -> MOffset {
        self.offset
    }

    pub const fn get_permission_flags(&self) -> MemoryPermissionFlags {
        self.permission_flags
    }

    pub fn set_permission_flags(&mut self, flags: MemoryPermissionFlags) {
        self.permission_flags = flags;
    }

    pub const fn get_memory_option_flags(&self) -> MemoryOptionFlags {
        self.option_flags
    }

    pub fn set_memory_option_flags(&mut self, flags: MemoryOptionFlags) {
        let _lock = self.lock.lock();
        self.option_flags = flags;
    }

    pub fn is_disabled(&self) -> bool {
        self.start_address.is_zero() && self.end_address.is_zero() && self.object.is_disabled()
    }

    pub fn set_disabled(&mut self) {
        let _lock = self.lock.lock();
        self.start_address = 0.into();
        self.end_address = 0.into();
        self.object.set_disabled();
    }

    pub fn set_up_to_be_root(&mut self, list_head: &mut PtrLinkedList<Self>) {
        let _lock = self.lock.lock();
        let ptr = self as *mut Self;
        self.list.set_ptr(ptr);
        let old_root = unsafe { list_head.get_first_entry_mut() };
        list_head.set_first_entry(Some(&mut self.list));
        if let Some(entry) = old_root {
            self.list.setup_to_be_root(&mut entry.list);
        }
    }

    pub fn get_object(&self) -> &VirtualMemoryObject {
        &self.object
    }

    pub fn get_object_mut(&mut self) -> &mut VirtualMemoryObject {
        &mut self.object
    }

    pub fn get_next_entry(&self) -> Option<&Self> {
        unsafe { self.list.get_next() }
    }

    pub fn get_next_entry_mut(&mut self) -> Option<&'static mut Self> {
        unsafe { self.list.get_next_mut() }
    }

    pub fn get_prev_entry(&self) -> Option<&Self> {
        unsafe { self.list.get_prev() }
    }
    pub fn get_prev_entry_mut(&mut self) -> Option<&'static mut Self> {
        unsafe { self.list.get_prev_mut() }
    }

    pub fn insert_after(&mut self /*must be chained*/, entry: &'static mut Self) {
        let _lock = self.lock.lock();
        if entry.list.is_invalid_ptr() {
            let ptr = entry as *mut Self;
            entry.list.set_ptr(ptr);
        }
        self.list.insert_after(&mut entry.list);
    }

    pub fn insert_before(&mut self /*must be chained*/, entry: &'static mut Self) {
        let _lock = self.lock.lock();
        if entry.list.is_invalid_ptr() {
            let ptr = entry as *mut Self;
            entry.list.set_ptr(ptr);
        }
        self.list.insert_before(&mut entry.list);
    }

    pub fn remove_from_list(&mut self, list: &mut PtrLinkedList<Self>) {
        self.list.remove_from_list(list);
    }

    pub fn adjust_entries(&'static mut self) -> &'static mut Self /*new root*/ {
        /* self should be root. */
        let mut new_root = self;
        while let Some(entry) = new_root.get_prev_entry_mut() {
            new_root = entry;
        }
        new_root
    }
}
