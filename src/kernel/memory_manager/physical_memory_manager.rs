//!
//! Physical Memory Manager
//!
//! 現時点では連結リスト管理だが、AVL-Treeなども実装してみたい

use super::data_type::{Address, MOrder, MPageOrder, MSize, PAddress};
use super::slab_allocator::pool_allocator::PoolAllocator;
use super::MemoryError;

use crate::arch::target_arch::paging::PAGE_SHIFT;

use crate::kernel::sync::spin_lock::IrqSaveSpinLockFlag;

pub struct PhysicalMemoryManager {
    lock: IrqSaveSpinLockFlag,
    memory_size: MSize,
    free_memory_size: MSize,
    first_entry: *mut MemoryEntry,
    free_list: [Option<*mut MemoryEntry>; Self::NUM_OF_FREE_LIST],
    memory_entry_pool: PoolAllocator<MemoryEntry>,
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
struct FreeListIter {
    entry: Option<*const MemoryEntry>,
}

/* ATTENTION: free_list's Iter(not normal next)*/
struct FreeListIterMut {
    entry: Option<*mut MemoryEntry>,
}

impl PhysicalMemoryManager {
    const NUM_OF_FREE_LIST: usize = 12;
    const POOL_THRESHOLD: usize = 3;

    pub const fn new() -> Self {
        Self {
            lock: IrqSaveSpinLockFlag::new(),
            memory_size: MSize::new(0),
            free_memory_size: MSize::new(0),
            free_list: [None; Self::NUM_OF_FREE_LIST],
            memory_entry_pool: PoolAllocator::new(),
            first_entry: core::ptr::null_mut(),
        }
    }

    pub fn get_memory_size(&self) -> MSize {
        self.memory_size
    }

    pub fn get_free_memory_size(&self) -> MSize {
        self.free_memory_size
    }

    pub fn add_memory_entry_pool(&mut self, pool_address: usize, pool_size: usize) {
        let _lock = self.lock.lock();
        unsafe { self.memory_entry_pool.add_pool(pool_address, pool_size) }
    }

    pub fn should_add_entry_pool(&self) -> bool {
        self.memory_entry_pool.get_count() <= Self::POOL_THRESHOLD
    }

    fn create_memory_entry(&mut self) -> Result<&'static mut MemoryEntry, MemoryError> {
        if let Ok(e) = self.memory_entry_pool.alloc() {
            e.set_enabled();
            e.init();
            Ok(e)
        } else {
            Err(MemoryError::EntryPoolRunOut)
        }
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
                return if entry.get_end_address() <= address {
                    Some(entry)
                } else {
                    entry.get_prev_entry()
                };
            }
        }
        entry.get_prev_entry()
    }

    fn define_used_memory(
        &mut self,
        start_address: PAddress,
        size: MSize,
        align_order: MOrder,
        target_entry: &mut Option<&mut MemoryEntry>,
    ) -> Result<(), MemoryError> {
        if size.is_zero() || self.free_memory_size < size {
            return Err(MemoryError::InvalidSize);
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
            return Err(MemoryError::InvalidAddress);
        };

        if entry.get_start_address() == start_address {
            if entry.get_end_address() == size.to_end_address(start_address) {
                /* Delete the entry */
                if entry.is_first_entry() {
                    if let Some(next) = entry.get_next_entry() {
                        self.first_entry = next as *mut _;
                    } else {
                        return Err(MemoryError::AddressNotAvailable);
                    }
                }
                self.unchain_entry_from_free_list(entry);
                entry.delete();
                self.memory_entry_pool.free_ptr(entry as *mut MemoryEntry);
                if target_entry.is_some() {
                    *target_entry = None;
                }
            } else {
                let old_size = entry.get_size();
                entry.set_range(start_address + size, entry.get_end_address());
                self.chain_entry_to_free_list(entry, Some(old_size));
            }
        } else if entry.get_end_address() == start_address {
            if size.to_usize() != 1 {
                return Err(MemoryError::InvalidAddress);
            }
            /* Allocate 1 byte of end_address */
            entry.set_range(entry.get_start_address(), start_address - MSize::new(1));
            self.chain_entry_to_free_list(entry, Some(entry.get_size() + size));
        } else if entry.get_end_address() == size.to_end_address(start_address) {
            let old_size = entry.get_size();
            entry.set_range(entry.get_start_address(), start_address - MSize::new(1));
            self.chain_entry_to_free_list(entry, Some(old_size));
        } else {
            let new_entry = self.create_memory_entry()?;
            let old_size = entry.get_size();
            new_entry.set_range(start_address + size, entry.get_end_address());
            entry.set_range(entry.get_start_address(), start_address - MSize::new(1));
            if let Some(next) = entry.get_next_entry() {
                new_entry.chain_after_me(next);
            }
            entry.chain_after_me(new_entry);
            self.chain_entry_to_free_list(entry, Some(old_size));
            self.chain_entry_to_free_list(new_entry, None);
        }
        self.free_memory_size -= size;
        return Ok(());
    }

    fn define_free_memory(
        &mut self,
        start_address: PAddress,
        size: MSize,
    ) -> Result<(), MemoryError> {
        if size.is_zero() {
            return Err(MemoryError::InvalidSize);
        }
        let entry = self
            .search_entry_previous_address_mut(start_address)
            .unwrap_or(unsafe { &mut *self.first_entry });
        let end_address = size.to_end_address(start_address);

        if entry.get_start_address() <= start_address && entry.get_end_address() >= end_address {
            /* already freed */
            return Err(MemoryError::InvalidAddress);
        } else if entry.get_end_address() >= start_address && !entry.is_first_entry() {
            /* Free duplicated area */
            return self.define_free_memory(
                entry.get_end_address() + MSize::new(1),
                MSize::from_address(entry.get_end_address() + MSize::new(1), end_address),
            );
        } else if entry.get_end_address() == end_address {
            /* Free duplicated area */
            /* entry may be first entry */
            return self.define_free_memory(start_address, size - entry.get_size());
        }

        let mut processed = false;
        let old_size = entry.get_size();
        let address_after_entry = entry.get_end_address() + MSize::new(1);

        if address_after_entry == start_address {
            entry.set_range(entry.get_start_address(), end_address);
            processed = true;
        }

        if entry.is_first_entry() && entry.get_start_address() == end_address + MSize::new(1) {
            entry.set_range(start_address, entry.get_end_address());
            processed = true;
        }

        if let Some(next) = entry.get_next_entry() {
            if next.get_start_address() <= start_address {
                assert!(!processed);
                return if next.get_end_address() >= end_address {
                    Err(MemoryError::InvalidAddress) /* already freed */
                } else {
                    self.define_free_memory(
                        next.get_end_address() + MSize::new(1),
                        end_address - next.get_end_address(),
                    )
                };
            }
            if next.get_start_address() == end_address + MSize::new(1) {
                let next_old_size = next.get_size();
                next.set_range(start_address, next.get_end_address());
                self.chain_entry_to_free_list(next, Some(next_old_size));
                processed = true;
            }

            if (next.get_start_address() == entry.get_end_address() + MSize::new(1))
                || (processed && address_after_entry >= next.get_start_address())
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
                if !next.is_enabled() {
                    self.memory_entry_pool.free(next);
                }
                return Ok(());
            }
            let new_entry = self.create_memory_entry()?;
            new_entry.set_range(start_address, end_address);
            if new_entry.get_end_address() < entry.get_start_address() {
                if let Some(prev_entry) = entry.get_prev_entry() {
                    assert!(prev_entry.get_end_address() < new_entry.get_start_address());
                    prev_entry.chain_after_me(new_entry);
                    new_entry.chain_after_me(entry);
                } else {
                    self.first_entry = new_entry as *mut _;
                    new_entry.chain_after_me(entry);
                }
            } else {
                next.set_prev_entry(new_entry);
                new_entry.set_next_entry(next);
                entry.chain_after_me(new_entry);
            }
            self.free_memory_size += size;
            self.chain_entry_to_free_list(entry, Some(old_size));
            self.chain_entry_to_free_list(new_entry, None);
            if !next.is_enabled() {
                /* Maybe needless */
                self.memory_entry_pool.free(next);
            }
            Ok(())
        } else {
            if processed {
                self.free_memory_size += size;
                self.chain_entry_to_free_list(entry, Some(old_size));
                return Ok(());
            }
            let new_entry = self.create_memory_entry()?;
            new_entry.set_range(start_address, end_address);
            if entry.get_end_address() < new_entry.get_start_address() {
                entry.chain_after_me(new_entry);
            } else {
                if let Some(prev_entry) = entry.get_prev_entry() {
                    assert!(prev_entry.get_end_address() < entry.get_start_address());
                    prev_entry.chain_after_me(new_entry);
                } else {
                    self.first_entry = new_entry as *mut _;
                }
                new_entry.chain_after_me(entry);
            }
            self.free_memory_size += size;
            self.chain_entry_to_free_list(entry, Some(old_size));
            self.chain_entry_to_free_list(new_entry, None);
            Ok(())
        }
    }

    pub fn alloc(&mut self, size: MSize, align_order: MOrder) -> Result<PAddress, MemoryError> {
        if size.is_zero() || self.free_memory_size <= size {
            return Err(MemoryError::InvalidSize);
        }
        let page_order = Self::size_to_page_order(size);
        let _lock = self.lock.lock();
        for i in page_order.to_usize()..Self::NUM_OF_FREE_LIST {
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
                    self.define_used_memory(
                        address_to_allocate,
                        size,
                        MOrder::new(0),
                        &mut Some(entry),
                    )?;
                    return Ok(address_to_allocate);
                }
            }
        }
        Err(MemoryError::AddressNotAvailable)
    }

    pub fn reserve_memory(
        &mut self,
        start_address: PAddress,
        size: MSize,
        align_order: MOrder,
    ) -> Result<(), MemoryError> {
        /* initializing use only */
        let _lock = self.lock.lock();
        self.define_used_memory(start_address, size, align_order, &mut None)
    }

    pub fn free(
        &mut self,
        start_address: PAddress,
        size: MSize,
        is_initializing: bool,
    ) -> Result<(), MemoryError> {
        let _lock = self.lock.lock();
        if self.memory_size < self.free_memory_size + size && !is_initializing {
            return Err(MemoryError::InvalidSize);
        }
        if self.memory_size.is_zero() {
            let first_entry = self.create_memory_entry()?;

            first_entry.init();
            first_entry.set_range(start_address, size.to_end_address(start_address));
            first_entry.set_enabled();
            self.chain_entry_to_free_list(first_entry, None);
            self.first_entry = first_entry;
            self.memory_size = size;
            self.free_memory_size = size;
        } else {
            self.define_free_memory(start_address, size)?;
            if self.memory_size < self.free_memory_size {
                self.memory_size = self.free_memory_size;
            }
        }
        return Ok(());
    }

    fn unchain_entry_from_free_list(&mut self, entry: &mut MemoryEntry) {
        let order = Self::size_to_page_order(entry.get_size());
        if self.free_list[order.to_usize()] == Some(entry as *mut _) {
            self.free_list[order.to_usize()] = entry.list_next;
        }
        entry.unchain_from_freelist();
    }

    fn chain_entry_to_free_list(&mut self, entry: &mut MemoryEntry, old_size: Option<MSize>) {
        let new_order = Self::size_to_page_order(entry.get_size());
        if let Some(old_size) = old_size {
            if old_size == entry.get_size() {
                return;
            }
            let old_order = Self::size_to_page_order(old_size);
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
                        if next_entry.get_size() >= entry.get_size() {
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
    fn size_to_page_order(size: MSize) -> MPageOrder {
        MPageOrder::from_offset(size, MPageOrder::new(Self::NUM_OF_FREE_LIST - 1))
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
    ) -> (PAddress, MSize) {
        if address.is_zero() {
            return (PAddress::new(0), size);
        }
        let align_size = align_order.to_offset().to_usize();
        let mask = !(align_size - 1);
        let aligned_address = PAddress::new(((address - MSize::new(1)) & mask) + align_size);
        assert!(aligned_address >= address);
        if size >= (aligned_address - address) {
            (aligned_address, size - (aligned_address - address))
        } else {
            (aligned_address, MSize::new(0))
        }
    }

    pub fn dump_memory_entry(&self) -> Result<(), ()> {
        let _lock = self.lock.try_lock()?;

        let mut entry = unsafe { &*self.first_entry };
        if !entry.is_enabled() {
            pr_info!("Root Entry is not enabled.");
            return Err(());
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
        return Ok(());
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

    pub fn list_iter(&self) -> FreeListIter {
        FreeListIter {
            entry: Some(self as *const _),
        }
    }

    pub fn list_iter_mut(&mut self) -> FreeListIterMut {
        FreeListIterMut {
            entry: Some(self as *mut _),
        }
    }
}

impl Iterator for FreeListIter {
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

impl Iterator for FreeListIterMut {
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
