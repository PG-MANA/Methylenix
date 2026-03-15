//!
//! Virtual Memory Object
//!
//! This manager indicates memory data information like vm_page

use super::super::data_type::{MIndex, PAddress};
use super::virtual_memory_page::VirtualMemoryPage;

use crate::kernel::collections::ptr_linked_list::PtrLinkedList;
use crate::kernel::sync::spin_lock::SpinLockFlag;

use core::mem::offset_of;

pub struct VirtualMemoryObject {
    pub lock: SpinLockFlag,
    reference_count: usize,
    object: VirtualMemoryObjectType,
    vm_page_list: PtrLinkedList<VirtualMemoryPage>,
    total_linked_page: usize,
}

enum VirtualMemoryObjectType {
    Normal,
    IoMap(PAddress),
    Shadow(*mut VirtualMemoryObject),
    None,
}

impl VirtualMemoryObject {
    pub const fn new() -> Self {
        Self {
            lock: SpinLockFlag::new(),
            object: VirtualMemoryObjectType::None,
            vm_page_list: PtrLinkedList::new(),
            total_linked_page: 0,
            reference_count: 0,
        }
    }

    pub fn set_disabled(&mut self) {
        assert!(self.lock.is_locked());
        self.object = VirtualMemoryObjectType::None
    }

    pub fn is_disabled(&self) -> bool {
        assert!(self.lock.is_locked());
        matches!(self.object, VirtualMemoryObjectType::None)
    }

    pub fn has_shadow_entry(&self) -> bool {
        assert!(self.lock.is_locked());
        matches!(self.object, VirtualMemoryObjectType::Shadow(_))
    }

    pub fn has_vm_page(&self) -> bool {
        assert!(self.lock.is_locked());
        if matches!(self.object, VirtualMemoryObjectType::None)
            || matches!(self.object, VirtualMemoryObjectType::IoMap(_))
        {
            assert!(self.vm_page_list.is_empty());
            true
        } else {
            self.vm_page_list.is_empty()
        }
    }

    pub fn set_shared_object(&mut self, target_object: &mut Self) {
        assert!(self.lock.is_locked());
        assert!(target_object.lock.is_locked());
        assert!(!target_object.has_shadow_entry());
        assert!(self.is_disabled());
        target_object.reference_count += 1;
        self.object = VirtualMemoryObjectType::Shadow(target_object);
    }

    pub fn unset_shared_object(&mut self, target_object: &mut Self) {
        assert!(self.lock.is_locked());
        assert!(target_object.lock.is_locked());
        if let VirtualMemoryObjectType::Shadow(s) = self.object {
            if s != (target_object as *mut _) {
                pr_err!("Invalid shadow object was given");
                return;
            }
            target_object.reference_count -= 1;
            self.object = VirtualMemoryObjectType::None;
        } else {
            pr_err!("Self is not shared object");
        }
    }

    pub fn get_shared_object(&self) -> Option<&'static mut Self> {
        assert!(self.lock.is_locked());
        if let VirtualMemoryObjectType::Shadow(s) = self.object {
            Some(unsafe { &mut *s })
        } else {
            None
        }
    }

    pub fn get_reference_count(&self) -> usize {
        assert!(self.lock.is_locked());
        self.reference_count
    }

    pub fn get_io_map_base_address(&self) -> Option<PAddress> {
        assert!(self.lock.is_locked());
        if let VirtualMemoryObjectType::IoMap(p) = self.object {
            Some(p)
        } else {
            None
        }
    }

    pub fn set_io_map_base_address(&mut self, address: PAddress) {
        assert!(self.lock.is_locked());
        assert!(matches!(self.object, VirtualMemoryObjectType::None));
        self.object = VirtualMemoryObjectType::IoMap(address);
    }

    pub fn add_vm_page(&mut self, p_index: MIndex, vm_page: &'static mut VirtualMemoryPage) {
        assert!(self.lock.is_locked());
        if matches!(self.object, VirtualMemoryObjectType::IoMap(_)) {
            pr_err!("Adding vm_page to io map is invalid");
            return;
        }
        vm_page.set_p_index(p_index);
        const OFFSET: usize = offset_of!(VirtualMemoryPage, list);
        if self.vm_page_list.is_empty() {
            if !self.has_shadow_entry() {
                assert_eq!(self.total_linked_page, 0);
            }
            if matches!(self.object, VirtualMemoryObjectType::None) {
                self.object = VirtualMemoryObjectType::Normal;
            }
            unsafe { self.vm_page_list.insert_head(&mut vm_page.list) };
        } else if let Some(first) = self
            .vm_page_list
            .get_first_entry(OFFSET)
            .map(|e| unsafe { &*e })
            && first.get_p_index() > p_index
        {
            unsafe { self.vm_page_list.insert_head(&mut vm_page.list) };
        } else {
            let mut cursor = unsafe { self.vm_page_list.cursor_front_mut(OFFSET) };
            while let Some(e) = cursor.current().map(|e| unsafe { &mut *e }) {
                if p_index < e.get_p_index() {
                    unsafe { cursor.insert_before(&mut vm_page.list) };
                    break;
                }
                unsafe { cursor.move_next() };
            }
            if !cursor.is_valid() {
                unsafe { self.vm_page_list.insert_tail(&mut vm_page.list) };
            }
        }
        self.total_linked_page += 1;
    }

    pub fn get_vm_page(&self, p_index: MIndex) -> Option<&VirtualMemoryPage> {
        assert!(self.lock.is_locked());
        if matches!(self.object, VirtualMemoryObjectType::None)
            || matches!(self.object, VirtualMemoryObjectType::IoMap(_))
        {
            return None;
        }
        for e in unsafe { self.vm_page_list.iter(offset_of!(VirtualMemoryPage, list)) } {
            if e.get_p_index() == p_index {
                return Some(e);
            }
        }
        None
    }

    pub fn get_vm_page_mut(&mut self, p_index: MIndex) -> Option<&mut VirtualMemoryPage> {
        assert!(self.lock.is_locked());
        if matches!(self.object, VirtualMemoryObjectType::None)
            || matches!(self.object, VirtualMemoryObjectType::IoMap(_))
        {
            return None;
        }
        for e in unsafe {
            self.vm_page_list
                .iter_mut(offset_of!(VirtualMemoryPage, list))
        } {
            if e.get_p_index() == p_index {
                return Some(e);
            }
        }
        None
    }

    pub fn remove_vm_page(
        &mut self,
        p_index: MIndex,
    ) -> Option<&'static mut VirtualMemoryPage /* removed page */> {
        assert!(self.lock.is_locked());
        if matches!(self.object, VirtualMemoryObjectType::None)
            || matches!(self.object, VirtualMemoryObjectType::IoMap(_))
        {
            return None;
        }
        let mut cursor = unsafe {
            self.vm_page_list
                .cursor_front_mut(offset_of!(VirtualMemoryPage, list))
        };
        while let Some(e) = cursor.current().map(|e| unsafe { &mut *e }) {
            if e.get_p_index() == p_index {
                unsafe { cursor.remove_current() };
                self.total_linked_page -= 1;
                return Some(e);
            } else if e.get_p_index() > p_index {
                break;
            }
            unsafe { cursor.move_next() };
        }
        None
    }

    pub fn take_vm_page(&mut self) -> Option<&'static mut VirtualMemoryPage /* the page taken */> {
        assert!(self.lock.is_locked());
        if matches!(self.object, VirtualMemoryObjectType::None)
            || matches!(self.object, VirtualMemoryObjectType::IoMap(_))
        {
            return None;
        }
        unsafe {
            self.vm_page_list
                .take_first_entry(offset_of!(VirtualMemoryPage, list))
        }
        .map(|p| {
            self.total_linked_page -= 1;
            unsafe { &mut *p }
        })
    }
}
