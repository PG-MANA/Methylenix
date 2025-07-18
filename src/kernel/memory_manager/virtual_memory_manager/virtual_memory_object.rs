//!
//! Virtual Memory Object
//!
//! This manager indicates memory data information like vm_page

use super::super::data_type::MIndex;
use super::virtual_memory_page::VirtualMemoryPage;

use crate::kernel::collections::ptr_linked_list::PtrLinkedList;
use crate::kernel::sync::spin_lock::SpinLockFlag;

use core::mem::offset_of;

pub struct VirtualMemoryObject {
    pub lock: SpinLockFlag,
    object: VirtualMemoryObjectType,
    linked_page: usize,
    reference_count: usize,
}

enum VirtualMemoryObjectType {
    Page(PtrLinkedList<VirtualMemoryPage>),
    Shadow(*mut VirtualMemoryObject),
    None,
}

impl VirtualMemoryObject {
    pub const fn new() -> Self {
        Self {
            lock: SpinLockFlag::new(),
            object: VirtualMemoryObjectType::None,
            linked_page: 0,
            reference_count: 0,
        }
    }

    pub fn set_disabled(&mut self) {
        self.object = VirtualMemoryObjectType::None
    }

    pub const fn is_disabled(&self) -> bool {
        matches!(self.object, VirtualMemoryObjectType::None)
    }

    pub const fn is_shadow_entry(&self) -> bool {
        matches!(self.object, VirtualMemoryObjectType::Shadow(_))
    }

    pub fn set_shared_object(&mut self, target_object: &mut Self) {
        assert!(!target_object.is_shadow_entry());
        assert!(self.is_disabled());
        target_object.reference_count += 1;
        self.object = VirtualMemoryObjectType::Shadow(target_object);
    }

    pub fn unset_shared_object(&mut self, target_object: &mut Self) {
        target_object.reference_count -= 1;
        self.object = VirtualMemoryObjectType::None;
    }

    pub fn get_shared_object(&self) -> Option<&'static mut Self> {
        if let VirtualMemoryObjectType::Shadow(s) = self.object {
            Some(unsafe { &mut *s })
        } else {
            None
        }
    }

    pub fn get_reference_count(&self) -> usize {
        self.reference_count
    }

    pub fn add_vm_page(&mut self, p_index: MIndex, vm_page: &'static mut VirtualMemoryPage) {
        if let VirtualMemoryObjectType::Page(list) = &mut self.object {
            vm_page.set_p_index(p_index);
            const OFFSET: usize = offset_of!(VirtualMemoryPage, list);
            if list.is_empty() {
                assert_eq!(self.linked_page, 0);
                let _lock = vm_page.lock.lock();
                unsafe { list.insert_head(&mut vm_page.list) };
            } else if let Some(first) = list.get_first_entry(OFFSET).map(|e| unsafe { &*e })
                && first.get_p_index() > p_index
            {
                let _lock = vm_page.lock.lock();
                let _first_entry_lock = first.lock.lock();
                unsafe { list.insert_head(&mut vm_page.list) };
            } else {
                let mut curosor = unsafe { list.cursor_front_mut(OFFSET) };
                while let Some(e) = curosor.current().map(|e| unsafe { &mut *e }) {
                    if p_index < e.get_p_index() {
                        let _lock = vm_page.lock.lock();
                        let prev = unsafe { &*e.list.get_prev(OFFSET).unwrap() };
                        let _prev_lock = prev.lock.lock();
                        unsafe { curosor.insert_before(&mut vm_page.list) };
                        break;
                    }
                    unsafe { curosor.move_next() };
                }
                if !curosor.is_valid() {
                    let _lock = vm_page.lock.lock();
                    let prev = list.get_last_entry(OFFSET).map(|e| unsafe { &*e }).unwrap();
                    let _prev_lock = prev.lock.lock();
                    unsafe { list.insert_tail(&mut vm_page.list) };
                }
            }
            self.linked_page += 1;
        } else if let VirtualMemoryObjectType::Shadow(_) = &self.object {
            unimplemented!()
        } else {
            let mut list = PtrLinkedList::<VirtualMemoryPage>::new();
            let _lock = vm_page.lock.lock();
            vm_page.set_p_index(p_index);
            unsafe { list.insert_head(&mut vm_page.list) };

            self.object = VirtualMemoryObjectType::Page(list);
            self.linked_page = 1;
        }
    }

    pub fn activate_all_page(&mut self) {
        if let VirtualMemoryObjectType::Page(list) = &mut self.object {
            for e in unsafe { list.iter_mut(offset_of!(VirtualMemoryPage, list)) } {
                e.activate();
            }
        }
    }

    pub fn get_vm_page(&self, p_index: MIndex) -> Option<&VirtualMemoryPage> {
        if let VirtualMemoryObjectType::Page(list) = &self.object {
            for e in unsafe { list.iter(offset_of!(VirtualMemoryPage, list)) } {
                if e.get_p_index() == p_index {
                    return Some(e);
                }
            }
        }
        None
    }

    pub fn get_vm_page_mut(&mut self, p_index: MIndex) -> Option<&mut VirtualMemoryPage> {
        if let VirtualMemoryObjectType::Page(list) = &mut self.object {
            for e in unsafe { list.iter_mut(offset_of!(VirtualMemoryPage, list)) } {
                if e.get_p_index() == p_index {
                    return Some(e);
                }
            }
        }
        None
    }

    pub fn remove_vm_page(
        &mut self,
        p_index: MIndex,
    ) -> Option<&'static mut VirtualMemoryPage /*removed page*/> {
        if let VirtualMemoryObjectType::Page(list) = &mut self.object {
            let mut cursor = unsafe { list.cursor_front_mut(offset_of!(VirtualMemoryPage, list)) };
            while let Some(e) = cursor.current().map(|e| unsafe { &mut *e }) {
                if e.get_p_index() == p_index {
                    unsafe { cursor.remove_current() };
                    return Some(e);
                } else if e.get_p_index() > p_index {
                    break;
                }
                unsafe { cursor.move_next() };
            }
        }
        None
    }
}
