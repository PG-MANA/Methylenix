/*
 * Physical Memory Manager
 * 現時点では連結リスト管理だが、AVL-Treeなども実装してみたい
 */
/*WARN: このコードはPhysicalMemoryManager全体がMutexで処理されることを前提としているので、メモリの並行アクセス性を完全に無視してできている*/

use arch::target_arch::paging::PAGE_SHIFT;

use core::mem;
use kernel::memory_manager::MemoryManager;

const MEMORY_ENTRY_SIZE: usize = mem::size_of::<MemoryEntry>();

pub struct PhysicalMemoryManager {
    memory_size: usize,
    free_memory_size: usize,
    first_entry: usize,
    free_list: [Option<usize>; Self::NUM_OF_FREE_LIST],
    memory_entry_pool: usize,
    memory_entry_pool_size: usize,
}

struct MemoryEntry {
    /*空きメモリ領域を所持している*/
    previous: Option<usize>,
    next: Option<usize>,
    list_prev: Option<usize>,
    list_next: Option<usize>,
    start: usize,
    end: usize,
    enabled: bool,
}

/* ATTENTION: free_list's Iter(not normal next)*/
struct MemoryEntryListIter {
    entry: Option<usize>,
}

/* ATTENTION: free_list's Iter(not normal next)*/
struct MemoryEntryListIterMut {
    entry: Option<usize>,
}

impl PhysicalMemoryManager {
    const NUM_OF_FREE_LIST: usize = 12;

    pub const fn new() -> PhysicalMemoryManager {
        PhysicalMemoryManager {
            memory_size: 0,
            free_memory_size: 0,
            free_list: [None; Self::NUM_OF_FREE_LIST],
            memory_entry_pool: 0,
            memory_entry_pool_size: 0,
            first_entry: 0,
        }
    }

    pub const fn get_free_memory_size(&self) -> usize {
        self.free_memory_size
    }

    pub fn set_memory_entry_pool(&mut self, free_address: usize, free_address_size: usize) {
        self.memory_entry_pool = free_address;
        self.memory_entry_pool_size = free_address_size;
        for i in 0..(free_address_size / MEMORY_ENTRY_SIZE) {
            unsafe {
                (*((free_address + i * MEMORY_ENTRY_SIZE) as *mut MemoryEntry)).set_disabled()
            }
        }
        self.first_entry = self.memory_entry_pool;
    }

    fn create_memory_entry(&mut self) -> Option<&'static mut MemoryEntry> {
        for i in 0..(self.memory_entry_pool_size / MEMORY_ENTRY_SIZE) {
            let entry = unsafe {
                &mut *((self.memory_entry_pool + i * MEMORY_ENTRY_SIZE) as *mut MemoryEntry)
            };
            if !entry.is_enabled() {
                entry.set_enabled();
                entry.init();
                return Some(entry);
            }
        }
        None
    }

    fn search_entry_containing_address_mut(
        &mut self,
        address: usize,
    ) -> Option<&'static mut MemoryEntry> {
        let mut entry = unsafe { &mut *(self.first_entry as *mut MemoryEntry) };
        while entry.get_start_address() < address && entry.get_end_address() < address {
            if let Some(t) = entry.get_next_entry() {
                entry = t;
            } else {
                return None;
            }
        }
        if address >= entry.get_start_address() && address <= entry.get_end_address() {
            return Some(entry);
        }
        None
    }

    fn search_entry_previous_address_mut(
        &mut self,
        address: usize,
    ) -> Option<&'static mut MemoryEntry> {
        /* addressの直前のMemoryEntryを探す */
        let mut entry = unsafe { &mut *(self.first_entry as *mut MemoryEntry) };
        while entry.get_start_address() < address {
            if let Some(t) = entry.get_next_entry() {
                entry = t;
            } else {
                if entry.get_end_address() <= address {
                    return Some(entry);
                }
                return None;
            }
        }
        entry.get_prev_entry()
    }

    fn define_used_memory(
        &mut self,
        start_address: usize,
        size: usize,
        align_order: usize,
        target_entry: Option<&mut MemoryEntry>,
    ) -> bool {
        if size == 0 || self.free_memory_size < size {
            return false;
        }
        if align_order != 0 {
            assert!(PAGE_SHIFT >= align_order);
            let (aligned_start_address, aligned_size) =
                Self::align_address_and_size(start_address, size, align_order);
            return self.define_used_memory(aligned_start_address, aligned_size, 0, target_entry);
        }
        let entry = if let Some(t) = target_entry {
            assert!(t.get_start_address() <= start_address);
            assert!(t.get_end_address() >= MemoryManager::size_to_end_address(start_address, size));
            t
        } else if let Some(t) = self.search_entry_containing_address_mut(start_address) {
            t
        } else {
            return false;
        };

        if entry.get_start_address() == start_address {
            if entry.get_end_address() == MemoryManager::size_to_end_address(start_address, size) {
                /* delete the entry */
                if entry.is_first_entry() {
                    if let Some(next) = entry.get_next_entry() {
                        self.first_entry = next as *mut _ as usize;
                    } else {
                        panic!("Memory ran out.");
                    }
                }
                self.unchain_entry_from_free_list(entry);
                entry.delete();
            } else {
                let old_size = entry.get_size();
                entry.set_range(start_address + size, entry.get_end_address());
                self.chain_entry_to_free_list(entry, old_size, false);
            }
        } else if entry.get_end_address() == start_address {
            if size != 1 {
                return false;
            }
            /* allocate 1 byte of end_address */
            entry.set_range(entry.get_start_address(), start_address - 1);
            self.chain_entry_to_free_list(entry, entry.get_size() + size, false);
        } else if entry.get_end_address() == MemoryManager::size_to_end_address(start_address, size)
        {
            let old_size = entry.get_size();
            entry.set_range(entry.get_start_address(), start_address - 1);
            self.chain_entry_to_free_list(entry, old_size, false);
        } else {
            let new_entry = if let Some(t) = self.create_memory_entry() {
                t
            } else {
                /* alloc bigger memory pool... */
                return false;
            };
            let old_size = entry.get_size();
            new_entry.set_range(start_address + size, entry.get_end_address());
            entry.set_range(entry.get_start_address(), start_address - 1);
            if let Some(next) = entry.get_next_entry() {
                new_entry.chain_after_me(next);
            }
            entry.chain_after_me(new_entry);
            self.chain_entry_to_free_list(entry, old_size, false);
            self.chain_entry_to_free_list(new_entry, new_entry.get_size(), true);
        }
        self.free_memory_size -= size;
        true
    }

    fn define_free_memory(&mut self, start_address: usize, size: usize) -> bool {
        if size == 0 {
            return false;
        }
        let entry = self
            .search_entry_previous_address_mut(start_address)
            .unwrap_or(unsafe { &mut *(self.first_entry as *mut MemoryEntry) });

        if entry.get_start_address() <= start_address
            && entry.get_end_address() >= MemoryManager::size_to_end_address(start_address, size)
        {
            /* already freed */
            return false;
        } else if entry.get_end_address() >= start_address && !entry.is_first_entry() {
            /* free duplicated area */
            return self.define_free_memory(
                entry.get_end_address() + 1,
                MemoryManager::address_to_size(
                    entry.get_end_address() + 1,
                    MemoryManager::size_to_end_address(start_address, size),
                ),
            );
        } else if entry.get_end_address() == MemoryManager::size_to_end_address(start_address, size)
        {
            /* free duplicated area */
            /* entry may be first entry */
            return self.define_free_memory(start_address, size - entry.get_size());
        } else {
            let mut processed = false;
            let old_size = entry.get_size();
            if entry.get_end_address() + 1 == start_address {
                entry.set_range(
                    entry.get_start_address(),
                    MemoryManager::size_to_end_address(start_address, size),
                );
                processed = true;
            }
            if entry.is_first_entry()
                && entry.get_start_address()
                    == MemoryManager::size_to_end_address(start_address, size) + 1
            {
                entry.set_range(start_address, entry.get_end_address());
                processed = true;
            }
            if let Some(next) = entry.get_next_entry() {
                if next.get_start_address()
                    == MemoryManager::size_to_end_address(start_address, size) + 1
                {
                    next.set_range(start_address, next.get_end_address());
                    processed = true;
                }
                if next.get_start_address() == entry.get_end_address() + 1 {
                    entry.set_range(entry.get_start_address(), next.get_end_address());
                    self.unchain_entry_from_free_list(next);
                    next.delete();
                }
                if processed {
                    self.free_memory_size += size;
                    self.chain_entry_to_free_list(entry, old_size, false);
                    return true;
                }
                let new_entry = if let Some(t) = self.create_memory_entry() {
                    t
                } else {
                    /* allocate bigger memory pool... */
                    return false;
                };
                new_entry.set_range(
                    start_address,
                    MemoryManager::size_to_end_address(start_address, size),
                );
                if entry.is_first_entry() && new_entry.get_end_address() < entry.get_start_address()
                {
                    self.first_entry = new_entry as *mut _ as usize;
                    new_entry.unset_prev_entry();
                    new_entry.chain_after_me(entry);
                } else {
                    next.set_prev_entry(new_entry);
                    new_entry.set_next_entry(next);
                    entry.chain_after_me(new_entry);
                }
                self.free_memory_size += size;
                self.chain_entry_to_free_list(new_entry, new_entry.get_size(), true);
                return true;
            } else {
                if processed {
                    self.free_memory_size += size;
                    self.chain_entry_to_free_list(entry, old_size, false);
                    return true;
                }
                let new_entry = if let Some(t) = self.create_memory_entry() {
                    t
                } else {
                    /* allocate bigger memory pool... */
                    return false;
                };
                new_entry.set_range(
                    start_address,
                    MemoryManager::size_to_end_address(start_address, size),
                );
                new_entry.unset_next_entry();
                entry.chain_after_me(new_entry);
                self.free_memory_size += size;
                self.chain_entry_to_free_list(new_entry, new_entry.get_size(), true);
                return true;
            }
        }
    }

    pub fn alloc(&mut self, size: usize, align_order: usize) -> Option<usize> {
        if size == 0 || self.free_memory_size <= size {
            return None;
        }
        let order = Self::size_to_order(size);
        for i in order..Self::NUM_OF_FREE_LIST {
            let first_entry = if let Some(t) = self.free_list[i] {
                unsafe { &mut *(t as *mut MemoryEntry) }
            } else {
                continue;
            };

            for entry in first_entry.list_iter_mut() {
                if entry.get_size() >= size {
                    let address_to_allocate = if align_order != 0 {
                        let (aligned_address, aligned_available_size) =
                            Self::align_address_and_available_size(
                                entry.get_start_address(),
                                entry.get_size(),
                                align_order,
                            );
                        if aligned_available_size < size {
                            continue;
                        }
                        aligned_address
                    } else {
                        entry.get_start_address()
                    };
                    return if self.define_used_memory(address_to_allocate, size, 0, Some(entry)) {
                        Some(address_to_allocate)
                    } else {
                        None
                    };
                }
            }
        }
        None
    }

    pub fn reserve_memory(
        &mut self,
        start_address: usize,
        size: usize,
        align_order: usize,
    ) -> bool {
        /* initializing use only */
        self.define_used_memory(start_address, size, align_order, None)
    }

    pub fn free(&mut self, start_address: usize, size: usize, is_initializing: bool) -> bool {
        if self.memory_size < self.free_memory_size + size && !is_initializing {
            return false;
        }
        if self.memory_size == 0 {
            if self.memory_entry_pool_size < MEMORY_ENTRY_SIZE {
                return false;
            }
            let first_entry = unsafe { &mut *(self.memory_entry_pool as *mut MemoryEntry) };
            first_entry.init();
            first_entry.set_range(
                start_address,
                MemoryManager::size_to_end_address(start_address, size),
            );
            first_entry.set_enabled();
            self.chain_entry_to_free_list(first_entry, first_entry.get_size(), true);
            self.memory_size = size;
            self.free_memory_size = size;
        } else {
            if !self.define_free_memory(start_address, size) {
                return false;
            }
            if self.memory_size < self.free_memory_size {
                self.memory_size = self.free_memory_size;
            }
        }
        return true;
    }

    fn unchain_entry_from_free_list(&mut self, entry: &mut MemoryEntry) {
        let order = Self::size_to_order(entry.get_size());
        if self.free_list[order] == Some(entry as *const _ as usize) {
            self.free_list[order] = entry.list_next;
        }
        entry.unchain_from_freelist();
    }

    fn chain_entry_to_free_list(
        &mut self,
        entry: &mut MemoryEntry,
        old_size: usize,
        entry_is_new: bool,
    ) {
        let old_order = Self::size_to_order(old_size);
        let new_order = Self::size_to_order(entry.get_size());
        if !entry_is_new {
            if old_order == new_order {
                return;
            }
            if self.free_list[old_order] == Some(entry as *const _ as usize) {
                self.free_list[old_order] = entry.list_next;
            }
            entry.unchain_from_freelist();
        }
        assert_eq!(entry.list_next, None);
        assert_eq!(entry.list_prev, None);

        if self.free_list[new_order].is_none() {
            self.free_list[new_order] = Some(entry as *const _ as usize);
        } else {
            let mut tail_entry =
                unsafe { &mut *(self.free_list[new_order].unwrap() as *mut MemoryEntry) };
            while let Some(next_entry) = tail_entry.list_next {
                tail_entry = unsafe { &mut *(next_entry as *mut MemoryEntry) };
            }
            tail_entry.list_next = Some(entry as *const _ as usize);
            entry.list_prev = Some(tail_entry as *const _ as usize);
        }
    }

    fn size_to_order(size: usize) -> usize {
        use core::cmp::min;
        min(
            MemoryManager::size_to_order(size),
            Self::NUM_OF_FREE_LIST - 1,
        )
    }

    fn align_address_and_size(
        address: usize,
        size: usize,
        align_order: usize,
    ) -> (usize /*address*/, usize /*size*/) {
        let align_size = 1 << align_order;
        let mask = !(align_size - 1);
        let aligned_address = address & mask;
        let aligned_size = ((size + (address - aligned_address) - 1) & mask) + align_size;
        (aligned_address, aligned_size)
    }

    fn align_address_and_available_size(
        address: usize,
        size: usize,
        align_order: usize,
    ) -> (usize /*address*/, usize /*size*/) {
        if address == 0 {
            (0, size)
        } else {
            let align_size = 1 << align_order;
            let mask = !(align_size - 1);
            let aligned_address = ((address - 1) & mask) + align_size;
            let aligned_available_size = size - (aligned_address - address);
            (aligned_address, aligned_available_size)
        }
    }

    pub fn dump_memory_entry(&self) {
        let mut entry = unsafe { &*(self.first_entry as *const MemoryEntry) };
        if !entry.is_enabled() {
            pr_info!("Root Entry is not enabled.");
            return;
        }
        kprintln!(
            "Start:0x{:X} Size:0x{:X}",
            entry.get_start_address(),
            MemoryManager::address_to_size(entry.get_start_address(), entry.get_end_address())
        );
        while let Some(t) = entry.get_next_entry() {
            entry = t;
            kprintln!(
                "Start:0x{:X} Size:0x{:X}",
                entry.get_start_address(),
                MemoryManager::address_to_size(entry.get_start_address(), entry.get_end_address())
            );
        }
        kprintln!("List:");
        for order in 0..Self::NUM_OF_FREE_LIST {
            if self.free_list[order].is_none() {
                continue;
            }
            let first_entry = unsafe { &*(self.free_list[order].unwrap() as *const MemoryEntry) };
            kprintln!("order {}:", order);
            for entry in first_entry.list_iter() {
                kprintln!(
                    " Start:0x{:X} Size:0x{:X}",
                    entry.get_start_address(),
                    MemoryManager::address_to_size(
                        entry.get_start_address(),
                        entry.get_end_address()
                    )
                );
            }
        }
    }
}

impl MemoryEntry {
    pub fn init(&mut self) {
        self.previous = None;
        self.next = None;
        self.list_prev = None;
        self.list_next = None;
    }

    pub fn delete(&mut self) {
        self.set_disabled();
        if let Some(previous) = self.get_prev_entry() {
            if let Some(next) = self.get_next_entry() {
                previous.chain_after_me(next);
            } else {
                previous.unset_next_entry();
            }
        } else {
            if let Some(next) = self.get_next_entry() {
                next.unset_prev_entry();
            } else {
                pr_warn!("Not chained entry was deleted.");
            }
        }
    }

    pub fn set_enabled(&mut self) {
        self.enabled = true;
    }

    pub fn set_disabled(&mut self) {
        self.enabled = false;
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_range(&mut self, start: usize, end: usize) {
        assert!(start < end);
        self.start = start;
        self.end = end;
    }

    pub fn get_start_address(&self) -> usize {
        self.start
    }

    pub fn get_end_address(&self) -> usize {
        self.end
    }

    pub fn get_prev_entry(&self) -> Option<&'static mut Self> {
        if let Some(previous) = self.previous {
            unsafe { Some(&mut *(previous as *mut Self)) }
        } else {
            None
        }
    }

    pub fn set_prev_entry(&mut self, prev: &mut Self) {
        self.previous = Some(prev as *mut _ as usize);
    }

    pub fn unset_prev_entry(&mut self) {
        self.previous = None;
    }

    pub fn get_next_entry(&self) -> Option<&'static mut Self> {
        if let Some(next) = self.next {
            unsafe { Some(&mut *(next as *mut Self)) }
        } else {
            None
        }
    }

    pub fn set_next_entry(&mut self, next: &mut Self) {
        self.next = Some(next as *mut _ as usize);
    }

    pub fn unset_next_entry(&mut self) {
        self.next = None;
    }

    pub fn get_size(&self) -> usize {
        self.end - self.start + 1
    }

    pub fn chain_after_me(&mut self, entry: &mut Self) {
        self.next = Some(entry as *mut Self as usize);
        unsafe {
            (&mut *(entry as *mut Self)).previous = Some(self as *mut Self as usize);
        }
    }

    pub fn is_first_entry(&self) -> bool {
        self.previous == None
    }

    pub fn unchain_from_freelist(&mut self) {
        if let Some(prev_address) = self.list_prev {
            let prev_entry = unsafe { &mut *(prev_address as *mut Self) };
            prev_entry.list_next = self.list_next;
        }
        if let Some(next_address) = self.list_next {
            let next_entry = unsafe { &mut *(next_address as *mut Self) };
            next_entry.list_prev = self.list_prev;
        }
        self.list_next = None;
        self.list_prev = None;
    }

    pub fn list_iter(&self) -> MemoryEntryListIter {
        MemoryEntryListIter {
            entry: Some(self as *const _ as usize),
        }
    }

    pub fn list_iter_mut(&mut self) -> MemoryEntryListIterMut {
        MemoryEntryListIterMut {
            entry: Some(self as *const _ as usize),
        }
    }
}

impl Iterator for MemoryEntryListIter {
    type Item = &'static MemoryEntry;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(address) = self.entry {
            let entry = unsafe { &*(address as *mut MemoryEntry) };
            self.entry = entry.list_next; /* ATTENTION: get free_list's next*/
            Some(entry)
        } else {
            None
        }
    }
}

impl Iterator for MemoryEntryListIterMut {
    type Item = &'static mut MemoryEntry;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(address) = self.entry {
            let entry = unsafe { &mut *(address as *mut MemoryEntry) };
            self.entry = entry.list_next; /* ATTENTION: get free_list's next*/
            Some(entry)
        } else {
            None
        }
    }
}
