/*
 * Virtual Memory Page Entry
 * structure for mapping virtual address to physical address
 */

use kernel::memory_manager::MemoryOptionFlags;

use kernel::ptr_linked_list::{PtrLinkedList, PtrLinkedListNode};

pub struct VirtualMemoryPage {
    list: PtrLinkedListNode<Self>,
    status: PageStatus,
    p_index: usize,
    physical_address: usize,
}

#[derive(Eq, PartialEq)]
enum PageStatus {
    Unswappable,
    Active,
    InActive,
    Free,
}

impl VirtualMemoryPage {
    pub fn new(physical_address: usize, p_index: usize) -> Self {
        Self {
            list: PtrLinkedListNode::new(),
            status: PageStatus::InActive,
            p_index,
            physical_address,
        }
    }

    pub fn set_page_status(&mut self, option: MemoryOptionFlags) {
        if option.wired() {
            self.status = PageStatus::Unswappable
        }
    }

    pub fn activate(&mut self) {
        assert!(self.status != PageStatus::Free);
        if self.status == PageStatus::InActive {
            self.status = PageStatus::Active;
        }
    }

    pub fn insert_after(&mut self, entry: &'static mut Self, p_index /*for entry*/: usize) {
        assert!(self.p_index < p_index);
        if let Some(next) = self.get_prev_entry() {
            assert!(next.p_index > p_index);
        }
        entry.p_index = p_index;
        /* add: radix tree */
        self._insert_after(entry);
    }

    fn _insert_after(&mut self, entry: &'static mut Self) {
        let ptr = self as *mut Self;
        self.list.set_ptr(ptr);
        let ptr = entry as *mut Self;
        entry.list.set_ptr(ptr);
        self.list.insert_after(&mut entry.list);
    }

    pub fn set_root(&mut self, p_index: usize, list: &mut PtrLinkedList<Self>) {
        self.p_index = p_index;
        let ptr = self as *mut Self;
        self.list.set_ptr(ptr);
        self.list.terminate_prev_entry();
        list.set_first_entry(&mut self.list);
        /*adjust tree*/
    }

    pub fn remove_from_list(&mut self) {
        self.list.remove_from_list();
    }

    pub const fn get_p_index(&self) -> usize {
        self.p_index
    }

    pub fn set_p_index(&mut self, p_index: usize) {
        assert!(self.list.get_next().is_none());
        assert!(self.list.get_prev().is_none());
        self.p_index = p_index;
    }

    pub fn get_next_entry(&self) -> Option<&Self> {
        self.list.get_next()
    }

    pub fn get_prev_entry(&self) -> Option<&Self> {
        self.list.get_prev()
    }

    pub fn get_next_entry_mut(&mut self) -> Option<&mut Self> {
        self.list.get_next_mut()
    }

    pub fn get_prev_entry_mut(&mut self) -> Option<&mut Self> {
        self.list.get_prev_mut()
    }

    pub const fn get_physical_address(&self) -> usize {
        self.physical_address
    }
}
