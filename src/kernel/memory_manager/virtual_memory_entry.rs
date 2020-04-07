/*
 * Virtual Memory Entry Chain
 */

use arch::target_arch::paging::MAX_VIRTUAL_ADDRESS;

use kernel::memory_manager::MemoryPermissionFlags;

use core::mem;

#[derive(Clone, Copy)] // Test
pub struct VirtualMemoryEntry {
    next_entry: Option<usize>,
    prev_entry: Option<usize>,
    start_address: usize,
    physical_start_address: usize,
    /*temporary*/
    end_address: usize,
    is_shared: bool,
    should_cow: bool,
    permission_flags: MemoryPermissionFlags,
}
// ADD: thread chain


impl VirtualMemoryEntry {
    pub const ENTRY_SIZE: usize = mem::size_of::<Self>();

    pub const fn new(
        vm_start_address: usize,
        vm_end_address: usize,
        physical_start_address: usize,
        permission: MemoryPermissionFlags,
    ) -> Self {
        Self {
            prev_entry: None,
            next_entry: None,
            start_address: vm_start_address,
            end_address: vm_end_address,
            physical_start_address,
            is_shared: false,
            should_cow: false,
            permission_flags: permission,
        }
    }

    pub fn get_vm_start_address(&self) -> usize {
        self.start_address
    }

    pub fn get_vm_end_address(&self) -> usize {
        self.end_address
    }

    pub fn get_physical_address(&self) -> usize {
        self.physical_start_address
    }

    pub fn is_disabled(&self) -> bool {
        self.start_address == 0 && self.end_address == 0 && self.physical_start_address == 0
    }

    pub fn set_disabled(&mut self) {
        self.start_address = 0;
        self.end_address = 0;
        self.physical_start_address = 0;
    }

    pub fn chain_after_me(&mut self, entry: &mut Self) {
        let old_next_entry = self.next_entry;
        self.next_entry = Some(entry as *mut Self as usize);
        entry.prev_entry = Some(self as *mut Self as usize);
        entry.next_entry = old_next_entry;
    }

    pub fn unchain(&mut self) {
        if let Some(prev) = self.prev_entry {
            unsafe { &mut *(prev as *mut Self) }.next_entry = self.next_entry;
        }
        if let Some(next) = self.next_entry {
            unsafe { &mut *(next as *mut Self) }.prev_entry = self.prev_entry;
        }
        self.prev_entry = None;
        self.next_entry = None;
    }

    pub fn insert_entry(&mut self, new_entry: &mut Self) -> bool {
        // self should be root.
        self._insert_entry(new_entry, self.end_address < new_entry.start_address)
    }

    fn _insert_entry(&mut self, new_entry: &mut Self, search_right: bool) -> bool {
        if search_right {
            if self.end_address < new_entry.start_address {
                if let Some(address) = self.next_entry {
                    let next_entry = unsafe { &mut *(address as *mut Self) };
                    next_entry._insert_entry(new_entry, true)
                } else {
                    new_entry.next_entry = None;
                    self.chain_after_me(new_entry);
                    true
                }
            } else if self.start_address > new_entry.end_address {
                new_entry.chain_after_me(self);
                true
            } else {
                false
            }
        } else {
            if self.start_address > new_entry.end_address {
                if let Some(address) = self.prev_entry {
                    let prev_entry = unsafe { &mut *(address as *mut Self) };
                    prev_entry._insert_entry(new_entry, false)
                } else {
                    new_entry.prev_entry = None;
                    new_entry.chain_after_me(self);
                    true
                }
            } else if self.end_address > new_entry.start_address {
                new_entry.chain_after_me(self);
                true
            } else {
                false
            }
        }
    }

    pub fn delete_entry(&mut self) -> bool {
        self.unchain();
        self.set_disabled();
        true
    }

    pub fn adjust_entries(&mut self) -> bool {
        // self should be root.
        true
    }

    pub fn find_usable_memory_area(&self, size: usize) -> Option<usize> {
        //self shoud be first entry
        if let Some(next) = self.next_entry {
            let next_entry = unsafe { &mut *(next as *mut VirtualMemoryEntry) };
            next_entry.find_usable_memory_area(size)
        } else if self.end_address + 1 + size >= MAX_VIRTUAL_ADDRESS {
            None
        } else {
            Some(self.end_address + 1)
        }
    }

    pub fn find_entry(&self, vm_start_address: usize) -> Option<&Self/*should be fixed*/> {
        // self should be root.
        if self.start_address == vm_start_address {
            Some(self)
        } else if self.start_address > vm_start_address {
            if let Some(address) = self.prev_entry {
                unsafe { &*(address as *const Self) }._find_entry(vm_start_address, false)
            } else {
                None
            }
        } else {
            if let Some(address) = self.next_entry {
                unsafe { &*(address as *const Self) }._find_entry(vm_start_address, true)
            } else {
                None
            }
        }
    }

    fn _find_entry(&self, vm_start_address: usize, search_right: bool) -> Option<&Self/*should be fixed*/> {
        if self.start_address == vm_start_address {
            Some(self)
        } else if self.start_address < vm_start_address && search_right {
            if let Some(address) = self.next_entry {
                unsafe { &*(address as *const Self) }._find_entry(vm_start_address, search_right)
            } else {
                None
            }
        } else if self.start_address > vm_start_address && !search_right {
            if let Some(address) = self.prev_entry {
                unsafe { &*(address as *const Self) }._find_entry(vm_start_address, search_right)
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn find_entry_mut(&mut self, vm_start_address: usize) -> Option<&mut Self/*should be fixed*/> {
        // self should be root.
        if self.start_address == vm_start_address {
            unsafe { Some(&mut *(self as *mut Self)) }
        } else if self.start_address > vm_start_address {
            if let Some(address) = self.prev_entry {
                unsafe { &mut *(address as *mut Self) }._find_entry_mut(vm_start_address, false)
            } else {
                None
            }
        } else {
            if let Some(address) = self.next_entry {
                unsafe { &mut *(address as *mut Self) }._find_entry_mut(vm_start_address, true)
            } else {
                None
            }
        }
    }

    fn _find_entry_mut(&mut self, vm_start_address: usize, search_right: bool) -> Option<&mut Self/*should be fixed*/> {
        if self.start_address == vm_start_address {
            unsafe { Some(&mut *(self as *mut Self)) }
        } else if self.start_address < vm_start_address && search_right {
            if let Some(address) = self.next_entry {
                unsafe { &mut *(address as *mut Self) }._find_entry_mut(vm_start_address, search_right)
            } else {
                None
            }
        } else if self.start_address > vm_start_address && !search_right {
            if let Some(address) = self.prev_entry {
                unsafe { &mut *(address as *mut Self) }._find_entry_mut(vm_start_address, search_right)
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn check_usable_address_range(&self, vm_start_address: usize, vm_end_address: usize) -> bool {
        self._check_usable_address_range(vm_start_address, vm_end_address, self.start_address <= vm_start_address)
    }

    fn _check_usable_address_range(&self, vm_start_address: usize, vm_end_address: usize, search_right: bool) -> bool {
        if (self.start_address <= vm_start_address && self.end_address >= vm_start_address) ||
            (self.start_address <= vm_end_address && self.end_address >= vm_end_address) {
            false
        } else if search_right && self.end_address < vm_start_address {
            if let Some(address) = self.next_entry {
                unsafe { &*(address as *const Self) }._check_usable_address_range(vm_start_address, vm_end_address, search_right)
            } else {
                true
            }
        } else if !search_right && self.start_address > vm_end_address {
            if let Some(address) = self.prev_entry {
                unsafe { &*(address as *const Self) }._check_usable_address_range(vm_start_address, vm_end_address, search_right)
            } else {
                true
            }
        } else {
            true // is it ok?
        }
    }
}
