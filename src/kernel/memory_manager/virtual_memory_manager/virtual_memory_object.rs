//!
//! Virtual Memory Object
//!
//! This manager indicates memory data information like vm_page

use super::virtual_memory_page::VirtualMemoryPage;
use crate::kernel::memory_manager::data_type::MIndex;
use crate::kernel::ptr_linked_list::PtrLinkedList;
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
            if list.get_first_entry_as_ptr().is_none() {
                assert_eq!(self.linked_page, 0);
                vm_page.setup_to_be_root(p_index, list);
                self.linked_page = 1;
                return;
            }
            if unsafe { list.get_first_entry() }.unwrap().get_p_index() > p_index {
                /* must change root */
                let root = unsafe { list.get_first_entry_mut() }.unwrap();
                let root_p_index = root.get_p_index();
                vm_page.setup_to_be_root(p_index, list);
                vm_page.insert_after(root, root_p_index);
                self.linked_page += 1;
            } else {
                for e in list.iter_mut() {
                    let e = unsafe { &mut *e };
                    if p_index < e.get_p_index() {
                        e.get_prev_entry_mut()
                            .unwrap()
                            .insert_after(vm_page, p_index);
                        self.linked_page += 1;
                        return;
                    } else if e.get_next_entry().is_none() {
                        e.insert_after(vm_page, p_index);
                        self.linked_page += 1;
                        return;
                    }
                }
                pr_err!("Can not insert vm_page.");
            }
        } else {
            let mut list = PtrLinkedList::<VirtualMemoryPage>::new();
            vm_page.setup_to_be_root(p_index, &mut list);
            self.object = VirtualMemoryObjectType::Page(list);
            self.linked_page = 1;
        }
    }

    pub fn activate_all_page(&mut self) {
        if let VirtualMemoryObjectType::Page(list) = &mut self.object {
            for e in list.iter_mut() {
                let e = unsafe { &mut *e };
                e.activate();
            }
        }
    }

    pub fn get_vm_page(&self, p_index: MIndex) -> Option<&VirtualMemoryPage> {
        if let VirtualMemoryObjectType::Page(list) = &self.object {
            for e in list.iter() {
                let e = unsafe { &*e };
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
            for e in list.iter_mut() {
                let e = unsafe { &mut *e };
                if e.get_p_index() == p_index {
                    e.remove_from_list(list);
                    return Some(e);
                } else if e.get_p_index() > p_index {
                    break;
                }
            }
        }
        None
    }
}
