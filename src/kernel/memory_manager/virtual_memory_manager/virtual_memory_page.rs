/*
 * Virtual Memory Page Entry
 * structure for mapping virtual address to physical address
 */

use crate::kernel::memory_manager::data_type::{MIndex, PAddress};
use crate::kernel::memory_manager::MemoryOptionFlags;
use crate::kernel::sync::spin_lock::SpinLockFlag;

use crate::kernel::ptr_linked_list::{PtrLinkedList, PtrLinkedListNode};

pub struct VirtualMemoryPage {
    lock: SpinLockFlag,
    list: PtrLinkedListNode<Self>,
    status: PageStatus,
    p_index: MIndex,
    physical_address: PAddress,
}

#[derive(Eq, PartialEq)]
enum PageStatus {
    Unswappable,
    Active,
    InActive,
    Free,
}

impl VirtualMemoryPage {
    pub fn new(physical_address: PAddress, p_index: MIndex) -> Self {
        Self {
            lock: SpinLockFlag::new(),
            list: PtrLinkedListNode::new(),
            status: PageStatus::InActive,
            p_index,
            physical_address,
        }
    }

    pub fn set_page_status(&mut self, option: MemoryOptionFlags) {
        if option.wired() {
            let _lock = self.lock.lock();
            self.status = PageStatus::Unswappable
        }
    }

    pub fn activate(&mut self) {
        let _lock = self.lock.lock();
        assert!(self.status != PageStatus::Free);
        if self.status == PageStatus::InActive {
            self.status = PageStatus::Active;
        }
    }

    pub fn insert_after(&mut self, entry: &'static mut Self, p_index /*for entry*/: MIndex) {
        let _lock = self.lock.lock();
        assert!(self.p_index < p_index);
        if let Some(next) = self.get_next_entry() {
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

    pub fn setup_to_be_root(&mut self, p_index: MIndex, list: &mut PtrLinkedList<Self>) {
        let _lock = self.lock.lock();
        self.p_index = p_index;
        let ptr = self as *mut Self;
        self.list.set_ptr(ptr);
        let old_root = unsafe { list.get_first_entry_mut() };
        list.set_first_entry(&mut self.list);
        if let Some(old_root) = old_root {
            self.list.setup_to_be_root(&mut old_root.list);
        }
        /*adjust tree*/
    }

    pub fn remove_from_list(&mut self) {
        let _lock = self.lock.lock();
        self.list.remove_from_list();
    }

    pub const fn get_p_index(&self) -> MIndex {
        self.p_index
    }

    pub fn set_p_index(&mut self, p_index: MIndex) {
        let _lock = self.lock.lock();
        assert!(self.list.get_next_as_ptr().is_none());
        assert!(self.list.get_prev_as_ptr().is_none());
        self.p_index = p_index;
    }

    pub fn get_next_entry(&self) -> Option<&Self> {
        unsafe { self.list.get_next() }
    }

    pub fn get_prev_entry(&self) -> Option<&Self> {
        unsafe { self.list.get_prev() }
    }

    pub fn get_next_entry_mut(&mut self) -> Option<&mut Self> {
        unsafe { self.list.get_next_mut() }
    }

    pub fn get_prev_entry_mut(&mut self) -> Option<&mut Self> {
        unsafe { self.list.get_prev_mut() }
    }

    pub const fn get_physical_address(&self) -> PAddress {
        self.physical_address
    }
}
