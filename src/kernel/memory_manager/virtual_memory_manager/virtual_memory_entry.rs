//!
//! Virtual Memory Entry Chain
//!

use super::super::data_type::{
    Address, MOffset, MSize, MemoryOptionFlags, MemoryPermissionFlags, VAddress,
};
use super::virtual_memory_object::VirtualMemoryObject;

use crate::kernel::collections::ptr_linked_list::PtrLinkedListNode;
use crate::kernel::sync::spin_lock::SpinLockFlag;

use core::mem::offset_of;

#[allow(dead_code)]
pub struct VirtualMemoryEntry {
    pub(super) list: PtrLinkedListNode<Self>,
    lock: SpinLockFlag,
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
        if let Some(next_entry) =
            unsafe { self.list.get_next(offset_of!(VirtualMemoryEntry, list)) }
        {
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
        self.start_address = VAddress::new(0);
        self.end_address = VAddress::new(0);
        self.object.set_disabled();
    }

    pub fn get_object(&self) -> &VirtualMemoryObject {
        &self.object
    }

    pub fn get_object_mut(&mut self) -> &mut VirtualMemoryObject {
        &mut self.object
    }
}
