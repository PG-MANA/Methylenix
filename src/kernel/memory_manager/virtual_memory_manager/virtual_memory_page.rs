//!
//! Virtual Memory Page Entry
//!
//! This structure contains mapping virtual address to physical address

use super::super::data_type::{MIndex, MemoryOptionFlags, PAddress};

use crate::kernel::collections::ptr_linked_list::PtrLinkedListNode;

pub struct VirtualMemoryPage {
    pub(super) list: PtrLinkedListNode<Self>,
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
            list: PtrLinkedListNode::new(),
            status: PageStatus::InActive,
            p_index,
            physical_address,
        }
    }

    pub fn set_page_status(&mut self, option: MemoryOptionFlags) {
        if option.is_wired() {
            self.status = PageStatus::Unswappable
        }
    }

    pub const fn is_activated(&self) -> bool {
        matches!(self.status, PageStatus::Active)
    }

    pub fn activate(&mut self) {
        assert!(self.status != PageStatus::Free);
        if self.status == PageStatus::InActive {
            self.status = PageStatus::Active;
        }
    }

    pub fn inactivate(&mut self) {
        if self.status == PageStatus::Active {
            self.status = PageStatus::InActive;
        }
    }

    pub const fn get_p_index(&self) -> MIndex {
        self.p_index
    }

    pub fn set_p_index(&mut self, p_index: MIndex) {
        assert!(!self.list.has_next());
        assert!(!self.list.has_prev());
        self.p_index = p_index;
    }

    pub const fn get_physical_address(&self) -> PAddress {
        self.physical_address
    }
}
