//!
//! Virtual Memory Object
//!
//! This manager indicates memory data information like vm_page

use super::virtual_memory_page::VirtualMemoryPage;
use crate::kernel::collections::ptr_linked_list::PtrLinkedList;
use crate::kernel::memory_manager::data_type::MIndex;
/*use crate::kernel::sync::spin_lock::Mutex;*/

pub struct VirtualMemoryObject {
    object: VirtualMemoryObjectType,
    linked_page: usize,
}

enum VirtualMemoryObjectType {
    Page(PtrLinkedList<VirtualMemoryPage>),
    None,
}

impl VirtualMemoryObject {
    pub const fn new() -> Self {
        Self {
            object: VirtualMemoryObjectType::None,
            linked_page: 0,
        }
    }

    pub fn set_disabled(&mut self) {
        self.object = VirtualMemoryObjectType::None
    }

    pub const fn is_disabled(&self) -> bool {
        matches!(self.object, VirtualMemoryObjectType::None)
    }

    pub fn add_vm_page(&mut self, p_index: MIndex, vm_page: &'static mut VirtualMemoryPage) {
        if let VirtualMemoryObjectType::Page(list) = &mut self.object {
            vm_page.set_p_index(p_index);
            const OFFSET: usize = offset_of!(VirtualMemoryPage, list);
            if list.is_empty() {
                assert_eq!(self.linked_page, 0);
                let _lock = vm_page.lock.lock();
                list.insert_head(&mut vm_page.list);
            } else if unsafe { list.get_first_entry(OFFSET) }
                .unwrap()
                .get_p_index()
                > p_index
            {
                let _lock = vm_page.lock.lock();
                let _first_entry_lock =
                    unsafe { list.get_first_entry(OFFSET) }.unwrap().lock.lock();
                list.insert_head(&mut vm_page.list);
            } else {
                for e in unsafe { list.iter_mut(OFFSET) } {
                    if p_index < e.get_p_index() {
                        let _lock = vm_page.lock.lock();
                        let _prev_lock = unsafe { e.list.get_prev(OFFSET) }.unwrap().lock.lock();
                        list.insert_before(&mut e.list, &mut vm_page.list);
                    } else if !e.list.has_next() {
                        let _lock = vm_page.lock.lock();
                        let _prev_lock = e.lock.lock();
                        list.insert_tail(&mut vm_page.list);
                    }
                }
            }
            self.linked_page += 1;
        } else {
            let mut list = PtrLinkedList::<VirtualMemoryPage>::new();
            let _lock = vm_page.lock.lock();
            vm_page.set_p_index(p_index);
            list.insert_head(&mut vm_page.list);

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
        return None;
    }

    pub fn remove_vm_page(
        &mut self,
        p_index: MIndex,
    ) -> Option<&'static mut VirtualMemoryPage /*removed page*/> {
        if let VirtualMemoryObjectType::Page(list) = &mut self.object {
            for e in unsafe { list.iter_mut(offset_of!(VirtualMemoryPage, list)) } {
                if e.get_p_index() == p_index {
                    list.remove(&mut e.list);
                    return Some(e);
                } else if e.get_p_index() > p_index {
                    break;
                }
            }
        }
        return None;
    }
}
