/*
 * 物理メモリマネージャー
 * 動作を簡略化させるためにページサイズのメモリ管理しかしない(つもり)
 * 現時点では連結リスト管理だが、AVL-Treeなども実装してみたい
*/
#![allow(dead_code)]
/*TODO: 排他管理*/
/*WARN: このコードはPhysicalMemoryManager全体がMutexで処理されることを前提としているので、メモリの並行アクセス性を完全に無視してできている*/

/*use(depending on arch)*/
use arch::target_arch::paging::{PAGE_SIZE, PAGE_MASK};

/*use(not depending on arch)*/
use core::mem;

const MEMORY_ENTRY_SIZE: usize = mem::size_of::<MemoryEntry>();

pub struct PhysicalMemoryManager {
    memory_size: usize,
    free_memory_size: usize,
    root: usize,
    //空いているメモリのリスト
    memory_entry_pool: usize,
    memory_entry_pool_size: usize,
}

struct MemoryEntry {
    /*空きメモリ領域を所持している*/
    previous: Option<usize>,
    next: Option<usize>,
    start: usize,
    end: usize,
    /*どちらもページ単位でしないといけない*/
    enabled: bool,
}


impl PhysicalMemoryManager {
    pub const fn new() -> PhysicalMemoryManager {
        PhysicalMemoryManager {
            memory_size: 0,
            free_memory_size: 0,
            memory_entry_pool: 0,
            memory_entry_pool_size: 0,
            root: 0,
        }
    }

    pub fn get_memory_entry_pool(&self) -> (usize, usize) {
        (self.memory_entry_pool, self.memory_entry_pool_size)
    }

    pub fn set_memory_entry_pool(&mut self, free_address: usize, free_address_size: usize) {
        self.memory_entry_pool = free_address;
        self.memory_entry_pool_size = free_address_size;
        for i in 0..(free_address_size / MEMORY_ENTRY_SIZE) {
            unsafe { (*((free_address + i * MEMORY_ENTRY_SIZE) as *mut MemoryEntry)).set_disabled() }
        }
        self.root = self.memory_entry_pool;
    }

    fn create_memory_entry(&self) -> Option<&'static mut MemoryEntry> {
        /*TODO: CLIなどのMutex処理*/
        for i in 0..(self.memory_entry_pool_size / MEMORY_ENTRY_SIZE) {
            let entry = unsafe { &mut *((self.memory_entry_pool + i * MEMORY_ENTRY_SIZE) as *mut MemoryEntry) };
            if !entry.is_enabled() {
                entry.set_enabled();
                entry.init();
                return Some(entry);
            }
        }
        None
    }

    fn search_contained_entry(&mut self, address: usize) -> Option<&'static mut MemoryEntry> {
        /*addressを含むMemoryEntryを探す*/
        let mut entry = unsafe { &mut *(self.root as *mut MemoryEntry) };
        loop {
            if entry.get_end_address() >= address && address >= entry.get_start_address() {
                return Some(entry);
            }
            if let Some(t) = entry.get_next_entry() {
                entry = t;
            } else {
                return None;
            }
        }
    }

    fn search_previous_entry(&self, address: usize) -> Option<&'static mut MemoryEntry> {
        /*addressの直前のMemoryEntryを探す*/
        let mut entry = unsafe { &mut *(self.root as *mut MemoryEntry) };
        loop {
            if entry.get_start_address() >= address {
                return entry.get_previous_entry();
            }
            if let Some(t) = entry.get_next_entry() {
                entry = t;
            } else {
                if entry.get_end_address() <= address {
                    return Some(entry);
                }
                return None;
            }
        }
    }

    pub fn define_used_memory(&mut self, start: usize, length/*length= start - end + 1*/: usize) -> bool {
        if length == 0 || self.free_memory_size < length {
            return false;
        }
        let entry = if let Some(t) = self.search_contained_entry(start) { t } else { return false; };
        if entry.get_start_address() == start {
            if entry.get_end_address() == start + length - 1 {
                //Memory Entryがまるごと消える
                if let Some(previous) = entry.get_previous_entry() {
                    if let Some(next) = entry.get_next_entry() {
                        previous.chain_after_me(next);
                    } else {
                        previous.terminate_chain();
                    }
                } else {
                    if let Some(next) = entry.get_next_entry() {
                        next.set_me_root();
                        self.root = next as *const _ as usize;
                    } else {
                        panic!("Physical Memory Manager is broken: there is no free memory.");/*現行では空きメモリ0を表現できない*/
                    }
                    entry.set_disabled();
                }
            } else {
                entry.set_range(start + length, entry.get_end_address());
            }
        } else if entry.get_end_address() == start {
            if length != 1 {
                return false;
            }
            entry.set_range(entry.get_start_address(), start - 1);
        } else if entry.get_end_address() == start + length - 1 {
            entry.set_range(entry.get_start_address(), start - 1);
        } else {
            let new_entry = if let Some(t) = self.create_memory_entry() { t } else { return false; };
            new_entry.set_range(start + length, entry.get_end_address());
            entry.set_range(entry.get_start_address(), start - 1);
            if let Some(next) = entry.get_next_entry() {
                new_entry.chain_after_me(next);
            }
            entry.chain_after_me(new_entry);
        }
        self.free_memory_size -= length;
        true
    }

    pub fn define_free_memory(&mut self, start: usize, length/*length= start - end + 1*/: usize) -> bool {
        if length == 0 {
            return false;
        }
        if self.memory_size == 0 {
            //未初期化
            let root = unsafe { &mut *(self.root as *mut MemoryEntry) };
            root.set_enabled();
            root.init();
            root.set_range(start, start + length - 1);
            self.free_memory_size += length;
        } else {
            self.free(start, length);
        }
        if self.free_memory_size > self.memory_size {
            self.memory_size = self.free_memory_size;
        }
        true
    }

    pub fn alloc(&mut self, size: usize, align: bool/*PAGE_SIZEでアラインするか*/) -> Option<usize> {
        if size == 0 /*|| size & (!PAGE_MASK) != 0 */ || self.free_memory_size <= size {
            return None;
        }
        let mut entry = unsafe { &mut *(self.root as *mut MemoryEntry) };
        loop {
            if entry.get_size() >= size {
                if align && entry.get_start_address() != 0 {
                    let aligned_addr = ((entry.get_start_address() - 1) & PAGE_MASK) + PAGE_SIZE;
                    if size <= entry.get_size() - (aligned_addr - entry.get_start_address()) {
                        break;
                    }
                } else {
                    break;
                }
            }
            if let Some(t) = entry.get_next_entry() {
                entry = t;
            } else {
                return None;
            }
        }
        let address = if align { ((entry.get_start_address() - 1) & PAGE_MASK) + PAGE_SIZE } else { entry.get_start_address() };
        if !self.define_used_memory(address, size) {
            None
        } else {
            Some(address)
        }
    }

    pub fn free(&mut self, address: usize, size: usize) -> bool {
        if size == 0 {
            return false;
        }
        let entry = self.search_previous_entry(address).unwrap_or(unsafe { &mut *(self.root as *mut MemoryEntry) });
        if entry.get_end_address() > address {
            if address + size == entry.get_start_address() {
                entry.set_range(address, entry.get_end_address());
            } else {
                let new_root = if let Some(t) = self.create_memory_entry() { t } else { return false; };
                new_root.set_range(address, address + size - 1);
                new_root.chain_after_me(unsafe { &mut *(self.root as *mut MemoryEntry) });
                self.root = new_root as *const _ as usize;
                self.free_memory_size += size;
                return true;
            }
        }
        if entry.get_end_address() + 1 == address {
            if let Some(next) = entry.get_next_entry() {
                if next.get_start_address() == address + size {
                    next.set_range(entry.get_start_address(), next.get_end_address());
                    entry.delete();
                }
            } else {
                entry.set_range(entry.get_start_address(), address + size - 1);
            }
            self.free_memory_size += size;
            return true;
        }
        if let Some(next) = entry.get_next_entry() {
            if next.get_start_address() == address + size {
                next.set_range(address, next.get_end_address());
                self.free_memory_size += size;
                return true;
            }
        }
        let new_entry = if let Some(t) = self.create_memory_entry() { t } else { return false; };
        new_entry.set_range(address, address + size - 1);
        if let Some(next) = entry.get_next_entry() {
            new_entry.chain_after_me(next);
        }
        entry.chain_after_me(new_entry);
        self.free_memory_size += size;
        true
    }

    pub fn dump_memory_entry(&self) {
        let mut entry = unsafe { &*(self.root as *const MemoryEntry) };
        if !entry.is_enabled() {
            println!("Root Entry is not enabled.");
            return;
        }
        println!("Root:start:{:X},end:{:X}", entry.get_start_address(), entry.get_end_address());
        while let Some(t) = entry.get_next_entry() {
            entry = t;
            println!(" Entry:start:{:X},end:{:X}", entry.get_start_address(), entry.get_end_address());
        }
    }
}

impl MemoryEntry {
    pub unsafe fn new_from_address(address: usize) -> &'static mut MemoryEntry {
        let entry = &mut *(address as *mut MemoryEntry);
        entry.init();
        entry
    }

    pub fn delete(&mut self) {
        self.set_disabled();
        if let Some(previous) = self.get_previous_entry() {
            if let Some(next) = self.get_next_entry() {
                previous.chain_after_me(next);
            } else {
                previous.next = None;
            }
        } else {
            panic!("Physical Memory Manager is broken: MemoryMap's chain is wrong.");
            /*つまりrootは削除不可*/
        }
    }

    pub fn init(&mut self) {
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

    pub fn set_range(&mut self, start: usize, end: usize) {
        self.start = start;
        self.end = end;
    }

    pub fn get_start_address(&self) -> usize {
        self.start
    }

    pub fn get_end_address(&self) -> usize {
        self.end
    }

    pub fn get_previous_entry(&self) -> Option<&'static mut Self> {
        if let Some(previous) = self.previous {
            unsafe { Some(&mut *(previous as *mut Self)) }
        } else {
            None
        }
    }

    pub fn get_next_entry(&self) -> Option<&'static mut Self> {
        if let Some(next) = self.next {
            unsafe { Some(&mut *(next as *mut Self)) }
        } else {
            None
        }
    }

    pub fn get_size(&self) -> usize {
        self.end - self.start + 1
    }

    pub fn chain_after_me(&mut self, entry: &mut Self) {
        self.next = Some(entry as *mut Self as usize);
        unsafe { (&mut *(entry as *mut Self)).previous = Some(self as *mut Self as usize); }
    }

    pub fn terminate_chain(&mut self) {
        self.next = None;
    }

    pub fn set_me_root(&mut self) {
        self.previous = None;
    }
}