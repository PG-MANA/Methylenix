//!
//! Physical Memory Manager
//!
//! 現時点では連結リスト管理だが、AVL-Treeなども実装してみたい
//! WARN: このコードはPhysicalMemoryManager全体がMutexで処理されることを前提としているので、メモリの並行アクセス性を完全に無視してできている

use crate::arch::target_arch::paging::PAGE_SHIFT;

use crate::kernel::memory_manager::data_type::{Address, MOrder, MPageOrder, MSize, PAddress};

const MEMORY_ENTRY_SIZE: usize = core::mem::size_of::<MemoryEntry>();

pub struct PhysicalMemoryManager {
    memory_size: MSize,
    free_memory_size: MSize,
    first_entry: *mut MemoryEntry,
    free_list: [Option<*mut MemoryEntry>; Self::NUM_OF_FREE_LIST],
    memory_entry_pool: usize,
    memory_entry_pool_size: usize,
}

struct MemoryEntry {
    /* Contains free memory area */
    previous: Option<*mut Self>,
    next: Option<*mut Self>,
    list_prev: Option<*mut Self>,
    list_next: Option<*mut Self>,
    start: PAddress,
    end: PAddress,
    enabled: bool,
}

/* ATTENTION: free_list's Iter(not normal next)*/
struct MemoryEntryListIter {
    entry: Option<*const MemoryEntry>,
}

/* ATTENTION: free_list's Iter(not normal next)*/
struct MemoryEntryListIterMut {
    entry: Option<*mut MemoryEntry>,
}

impl PhysicalMemoryManager {
    const NUM_OF_FREE_LIST: usize = 12;

    pub const fn new() -> Self {
        Self {
            memory_size: MSize::new(0),
            free_memory_size: MSize::new(0),
            free_list: [None; Self::NUM_OF_FREE_LIST],
            memory_entry_pool: 0,
            memory_entry_pool_size: 0,
            first_entry: core::ptr::null_mut(),
        }
    }

    pub const fn get_free_memory_size(&self) -> MSize {
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
        self.first_entry = self.memory_entry_pool as *mut _;
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
        address: PAddress,
    ) -> Option<&'static mut MemoryEntry> {
        let mut entry = unsafe { &mut *self.first_entry };
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
        address: PAddress,
    ) -> Option<&'static mut MemoryEntry> {
        let mut entry = unsafe { &mut *self.first_entry };
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
        start_address: PAddress,
        size: MSize,
        align_order: MOrder,
        target_entry: Option<&mut MemoryEntry>,
    ) -> bool {
        if size.is_zero() || self.free_memory_size < size {
            return false;
        }
        if !align_order.is_zero() {
            assert!(PAGE_SHIFT >= align_order.to_usize());
            let (aligned_start_address, aligned_size) =
                Self::align_address_and_size(start_address, size, align_order);
            return self.define_used_memory(
                aligned_start_address,
                aligned_size,
                MOrder::new(0),
                target_entry,
            );
        }
        let entry = if let Some(t) = target_entry {
            assert!(t.get_start_address() <= start_address);
            assert!(t.get_end_address() >= size.to_end_address(start_address));
            t
        } else if let Some(t) = self.search_entry_containing_address_mut(start_address) {
            t
        } else {
            return false;
        };

        if entry.get_start_address() == start_address {
            if entry.get_end_address() == size.to_end_address(start_address) {
                /* Delete the entry */
                if entry.is_first_entry() {
                    if let Some(next) = entry.get_next_entry() {
                        self.first_entry = next as *mut _;
                    } else {
                        panic!("Memory ran out.");
                    }
                }
                self.unchain_entry_from_free_list(entry);
                entry.delete();
            } else {
                let old_size = entry.get_size();
                entry.set_range(start_address + size, entry.get_end_address());

                self.chain_entry_to_free_list(entry, Some(old_size));
            }
        } else if entry.get_end_address() == start_address {
            if size.to_usize() != 1 {
                return false;
            }
            /* Allocate 1 byte of end_address */
            entry.set_range(entry.get_start_address(), start_address - MSize::from(1));
            self.chain_entry_to_free_list(entry, Some(entry.get_size() + size));
        } else if entry.get_end_address() == size.to_end_address(start_address) {
            let old_size = entry.get_size();
            entry.set_range(entry.get_start_address(), start_address - MSize::from(1));
            self.chain_entry_to_free_list(entry, Some(old_size));
        } else {
            let new_entry = if let Some(t) = self.create_memory_entry() {
                t
            } else {
                /* TODO: Alloc bigger memory pool... */
                return false;
            };
            let old_size = entry.get_size();
            new_entry.set_range(start_address + size, entry.get_end_address());
            entry.set_range(entry.get_start_address(), start_address - MSize::from(1));
            if let Some(next) = entry.get_next_entry() {
                new_entry.chain_after_me(next);
            }
            entry.chain_after_me(new_entry);
            self.chain_entry_to_free_list(entry, Some(old_size));
            self.chain_entry_to_free_list(new_entry, None);
        }
        self.free_memory_size -= size;
        true
    }

    fn define_free_memory(&mut self, start_address: PAddress, size: MSize) -> bool {
        if size.is_zero() {
            return false;
        }
        let entry = self
            .search_entry_previous_address_mut(start_address)
            .unwrap_or(unsafe { &mut *self.first_entry });

        if entry.get_start_address() <= start_address
            && entry.get_end_address() >= size.to_end_address(start_address)
        {
            /* already freed */
            false
        } else if entry.get_end_address() >= start_address && !entry.is_first_entry() {
            /* Free duplicated area */
            self.define_free_memory(
                entry.get_end_address() + MSize::from(1),
                MSize::from_address(
                    entry.get_end_address() + MSize::from(1),
                    size.to_end_address(start_address),
                ),
            )
        } else if entry.get_end_address() == size.to_end_address(start_address) {
            /* Free duplicated area */
            /* entry may be first entry */
            self.define_free_memory(start_address, size - entry.get_size())
        } else {
            let mut processed = false;
            let old_size = entry.get_size();
            if entry.get_end_address() + MSize::from(1) == start_address {
                entry.set_range(
                    entry.get_start_address(),
                    size.to_end_address(start_address),
                );
                processed = true;
            }
            if entry.is_first_entry()
                && entry.get_start_address() == size.to_end_address(start_address) + MSize::from(1)
            {
                entry.set_range(start_address, entry.get_end_address());
                processed = true;
            }
            if let Some(next) = entry.get_next_entry() {
                if next.get_start_address() == size.to_end_address(start_address) + MSize::from(1) {
                    let next_old_size = next.get_size();
                    next.set_range(start_address, next.get_end_address());
                    self.chain_entry_to_free_list(next, Some(next_old_size));
                    processed = true;
                }
                if (next.get_start_address() == entry.get_end_address() + MSize::from(1))
                    || (processed
                        && (entry.get_end_address() + MSize::from(1)) >= next.get_start_address())
                {
                    entry.set_range(
                        entry.get_start_address(),
                        core::cmp::max(entry.get_end_address(), next.get_end_address()),
                    );

                    self.unchain_entry_from_free_list(next);
                    next.delete();
                }
                if processed {
                    self.free_memory_size += size;
                    self.chain_entry_to_free_list(entry, Some(old_size));
                    return true;
                }
                let new_entry = if let Some(t) = self.create_memory_entry() {
                    t
                } else {
                    /* TODO: Alloc bigger memory pool... */
                    return false;
                };
                new_entry.set_range(start_address, size.to_end_address(start_address));
                if entry.is_first_entry() && new_entry.get_end_address() < entry.get_start_address()
                {
                    self.first_entry = new_entry as *mut _;
                    new_entry.unset_prev_entry();
                    new_entry.chain_after_me(entry);
                } else {
                    next.set_prev_entry(new_entry);
                    new_entry.set_next_entry(next);
                    entry.chain_after_me(new_entry);
                }
                self.free_memory_size += size;
                self.chain_entry_to_free_list(entry, Some(old_size));
                self.chain_entry_to_free_list(new_entry, None);
                true
            } else {
                if processed {
                    self.free_memory_size += size;
                    self.chain_entry_to_free_list(entry, Some(old_size));
                    return true;
                }
                let new_entry = if let Some(t) = self.create_memory_entry() {
                    t
                } else {
                    /* TODO: Alloc bigger memory pool... */
                    return false;
                };
                new_entry.set_range(start_address, size.to_end_address(start_address));
                new_entry.unset_next_entry();
                entry.chain_after_me(new_entry);
                self.free_memory_size += size;
                self.chain_entry_to_free_list(entry, Some(old_size));
                self.chain_entry_to_free_list(new_entry, None);
                true
            }
        }
    }

    pub fn alloc(&mut self, size: MSize, align_order: MOrder) -> Option<PAddress> {
        if size.is_zero() || self.free_memory_size <= size {
            return None;
        }
        let order = Self::size_to_order(size);
        for i in order.to_usize()..Self::NUM_OF_FREE_LIST {
            let first_entry = if let Some(t) = self.free_list[i] {
                unsafe { &mut *t }
            } else {
                continue;
            };

            for entry in first_entry.list_iter_mut() {
                if entry.get_size() >= size {
                    let address_to_allocate = if !align_order.is_zero() {
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
                    return if self.define_used_memory(
                        address_to_allocate,
                        size,
                        MOrder::new(0),
                        Some(entry),
                    ) {
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
        start_address: PAddress,
        size: MSize,
        align_order: MOrder,
    ) -> bool {
        /* initializing use only */
        self.define_used_memory(start_address, size, align_order, None)
    }

    pub fn free(&mut self, start_address: PAddress, size: MSize, is_initializing: bool) -> bool {
        if self.memory_size < self.free_memory_size + size && !is_initializing {
            return false;
        }
        if self.memory_size.is_zero() {
            if self.memory_entry_pool_size < MEMORY_ENTRY_SIZE {
                return false;
            }
            let first_entry = unsafe { &mut *(self.memory_entry_pool as *mut MemoryEntry) };
            first_entry.init();
            first_entry.set_range(start_address, size.to_end_address(start_address));
            first_entry.set_enabled();
            self.chain_entry_to_free_list(first_entry, None);
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
        if self.free_list[order.to_usize()] == Some(entry as *mut _) {
            self.free_list[order.to_usize()] = entry.list_next;
        }
        entry.unchain_from_freelist();
    }

    fn chain_entry_to_free_list(&mut self, entry: &mut MemoryEntry, old_size: Option<MSize>) {
        let new_order = Self::size_to_order(entry.get_size());
        if let Some(old_size) = old_size {
            let old_order = Self::size_to_order(old_size);
            if old_order == new_order {
                return;
            }
            if self.free_list[old_order.to_usize()] == Some(entry as *mut _) {
                self.free_list[old_order.to_usize()] = entry.list_next;
            }
            entry.unchain_from_freelist();
        }
        assert_eq!(entry.list_next, None);
        assert_eq!(entry.list_prev, None);

        if self.free_list[new_order.to_usize()].is_none() {
            self.free_list[new_order.to_usize()] = Some(entry as *mut _);
        } else {
            let mut list_entry: &mut MemoryEntry =
                unsafe { &mut *self.free_list[new_order.to_usize()].unwrap() };
            if list_entry.get_size() >= entry.get_size() {
                list_entry.list_prev = Some(entry as *mut _);
                entry.list_next = Some(list_entry as *mut _);
                self.free_list[new_order.to_usize()] = Some(entry as *mut _);
            } else {
                loop {
                    if let Some(next_entry) =
                        list_entry.list_next.and_then(|n| Some(unsafe { &mut *n }))
                    {
                        if next_entry.get_size() > entry.get_size() {
                            list_entry.list_next = Some(entry as *mut _);
                            entry.list_prev = Some(list_entry as *mut _);
                            entry.list_next = Some(next_entry as *mut _);
                            next_entry.list_prev = Some(entry as *mut _);
                            break;
                        }
                        list_entry = next_entry;
                    } else {
                        list_entry.list_next = Some(entry as *mut _);
                        entry.list_prev = Some(list_entry as *mut _);
                        break;
                    }
                }
            }
        }
    }

    #[inline]
    fn size_to_order(size: MSize) -> MOrder {
        size.to_order(Some(MOrder::new(Self::NUM_OF_FREE_LIST - 1)))
    }

    #[inline]
    /*const*/
    fn align_address_and_size(
        address: PAddress,
        size: MSize,
        align_order: MOrder,
    ) -> (PAddress /* address */, MSize /* size */) {
        let align_size = align_order.to_offset();
        let mask = !(align_size.to_usize() - 1);
        let aligned_address = address.to_usize() & mask;
        let aligned_size = MSize::new(
            ((size.to_usize() + (address.to_usize() - aligned_address) - 1) & mask)
                + align_size.to_usize(),
        );
        (PAddress::new(aligned_address), aligned_size)
    }

    #[inline]
    /*const*/
    fn align_address_and_available_size(
        address: PAddress,
        size: MSize,
        align_order: MOrder,
    ) -> (PAddress /* address */, MSize /* size */) {
        if address.is_zero() {
            (PAddress::new(0), size)
        } else {
            /* THINKING: Better algorithm */
            let align_size = align_order.to_offset().to_usize();
            let mask = !(align_size - 1);
            let mut aligned_address = ((address.to_usize() - 1) & mask) + align_size;
            let mut aligned_available_size = if aligned_address >= address.to_usize() {
                size.to_usize() - (aligned_address - address.to_usize())
            } else {
                size.to_usize() + (address.to_usize() - aligned_address)
            };
            while aligned_address < address.to_usize() {
                if aligned_available_size < align_size {
                    return (PAddress::new(aligned_address), MSize::new(0));
                }
                aligned_address += align_size;
                aligned_available_size -= align_size;
            }
            (
                PAddress::new(aligned_address),
                MSize::new(aligned_available_size),
            )
        }
    }

    pub fn dump_memory_entry(&self) {
        let mut entry = unsafe { &*self.first_entry };
        if !entry.is_enabled() {
            pr_info!("Root Entry is not enabled.");
            return;
        }
        kprintln!(
            "Start:{:>#16X}, Size:{:>#16X}",
            entry.get_start_address().to_usize(),
            MSize::from_address(entry.get_start_address(), entry.get_end_address()).to_usize()
        );
        while let Some(t) = entry.get_next_entry() {
            entry = t;
            kprintln!(
                "Start:{:>#16X}, Size:{:>#16X}",
                entry.get_start_address().to_usize(),
                MSize::from_address(entry.get_start_address(), entry.get_end_address()).to_usize()
            );
        }
        kprintln!("List:");
        for order in 0..Self::NUM_OF_FREE_LIST {
            if self.free_list[order].is_none() {
                continue;
            }
            let first_entry = unsafe { &*self.free_list[order].unwrap() };
            kprintln!("Order {}:", order);
            for entry in first_entry.list_iter() {
                kprintln!(
                    " Start:{:>#16X}, Size:{:>#16X}",
                    entry.get_start_address().to_usize(),
                    MSize::from_address(entry.get_start_address(), entry.get_end_address())
                        .to_usize()
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
        } else if let Some(next) = self.get_next_entry() {
            next.unset_prev_entry();
        } else {
            pr_warn!("Not chained entry was deleted.");
        }
        self.previous = None;
        self.next = None;
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

    pub fn set_range(&mut self, start: PAddress, end: PAddress) {
        assert!(start < end);
        self.start = start;
        self.end = end;
    }

    pub fn get_start_address(&self) -> PAddress {
        self.start
    }

    pub fn get_end_address(&self) -> PAddress {
        self.end
    }

    pub fn get_prev_entry(&self) -> Option<&'static mut Self> {
        if let Some(previous) = self.previous {
            unsafe { Some(&mut *previous) }
        } else {
            None
        }
    }

    pub fn set_prev_entry(&mut self, prev: &mut Self) {
        self.previous = Some(prev as *mut _);
    }

    pub fn unset_prev_entry(&mut self) {
        self.previous = None;
    }

    pub fn get_next_entry(&self) -> Option<&'static mut Self> {
        if let Some(next) = self.next {
            unsafe { Some(&mut *next) }
        } else {
            None
        }
    }

    pub fn set_next_entry(&mut self, next: &mut Self) {
        self.next = Some(next as *mut _);
    }

    pub fn unset_next_entry(&mut self) {
        self.next = None;
    }

    pub fn get_size(&self) -> MSize {
        MSize::from_address(self.start, self.end)
    }

    pub fn chain_after_me(&mut self, entry: &mut Self) {
        self.next = Some(entry as *mut _);
        entry.previous = Some(self as *mut _);
    }

    pub fn is_first_entry(&self) -> bool {
        self.previous == None
    }

    pub fn unchain_from_freelist(&mut self) {
        if let Some(prev_address) = self.list_prev {
            let prev_entry = unsafe { &mut *prev_address };
            prev_entry.list_next = self.list_next;
        }
        if let Some(next_address) = self.list_next {
            let next_entry = unsafe { &mut *next_address };
            next_entry.list_prev = self.list_prev;
        }
        self.list_next = None;
        self.list_prev = None;
    }

    pub fn list_iter(&self) -> MemoryEntryListIter {
        MemoryEntryListIter {
            entry: Some(self as *const _),
        }
    }

    pub fn list_iter_mut(&mut self) -> MemoryEntryListIterMut {
        MemoryEntryListIterMut {
            entry: Some(self as *mut _),
        }
    }
}

impl Iterator for MemoryEntryListIter {
    type Item = &'static MemoryEntry;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(address) = self.entry {
            let entry = unsafe { &*(address as *mut MemoryEntry) };
            self.entry = entry.list_next.and_then(|e| Some(e as *const _)); /* ATTENTION: get **free_list's** next */
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
            self.entry = entry.list_next; /* ATTENTION: get **free_list's** next */
            Some(entry)
        } else {
            None
        }
    }
}
