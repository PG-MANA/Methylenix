/*
メモリ管理システム
    とりあえず、MemManは4KBごとに空き、使用中か管理して詳しいことはページングでと考えてる
    メモリ領域がほしい(要求)=>まずあいているメモリをMemManで探しリアルアドレスをとってくる=>それを元にページを作成=>返す
    だといいかな。
    この実装は色んな本やサイトを参考にした書き方なのでいつか再実装する。
    というより土壇場実装なのでかなり危ない。
*/

mod physical_memory_manager;

use arch::target_arch::paging::PAGE_SIZE;

use kernel::drivers::multiboot::MultiBootInformation;
use kernel::memory_manager::physical_memory_manager::PhysicalMemoryManager;
use arch::x86_64::paging::PAGE_MASK;

pub struct MemoryManager {
    physical_memory_manager: PhysicalMemoryManager,
}

impl MemoryManager {
    pub fn new(multiboot_info: &MultiBootInformation) -> MemoryManager {
        let kernel_loader_range = (|m: &MultiBootInformation| {
            let max_entry = m.elf_info.clone().max_by_key(|s| s.addr()).unwrap();
            (m.elf_info.clone().min_by_key(|s| s.addr()).unwrap().addr(), max_entry.addr() + max_entry.size())
        })(multiboot_info);
        let memory_entry_pool_size = PAGE_SIZE * 2;
        let mut memory_entry_pool_address = 0usize;
        for entry in multiboot_info.memory_map_info.clone() {
            if entry.m_type != 1 {
                continue;
            }
            let mut aligned_address = if entry.addr != 0 { ((entry.addr - 1) as usize & PAGE_MASK) + PAGE_SIZE } else { 0 };
            let mut free_address_size = entry.length as usize - (aligned_address - entry.addr as usize);
            loop {
                if free_address_size < memory_entry_pool_size {
                    break;
                }
                if (aligned_address + memory_entry_pool_size <= kernel_loader_range.0 ||
                    aligned_address >= kernel_loader_range.1) &&
                    (aligned_address + memory_entry_pool_address <= multiboot_info.address ||
                        aligned_address >= multiboot_info.size + multiboot_info.size) {
                    /*Kernel Loaderはメモリマップに記載されてないので用意するメモリ領域の端がそれらに食い込まないかチェック*/
                    /*このチェックではELF飛び飛びに対応できてないけどいいや*/
                    memory_entry_pool_address = aligned_address;
                    break;
                }
                aligned_address += PAGE_SIZE;
                free_address_size -= PAGE_SIZE;
            }
            if free_address_size >= memory_entry_pool_size {
                break;
            }
        }
        let mut phy_memory_manager = PhysicalMemoryManager::new();
        phy_memory_manager.set_memory_entry_pool(memory_entry_pool_address, memory_entry_pool_size);
        for entry in multiboot_info.memory_map_info.clone() {
            /*2回回すのは少し気が引けるけど...*/
            if entry.m_type == 1 {
                phy_memory_manager.define_free_memory(entry.addr as usize, entry.length as usize);
                //PAGE_SIZE長に切りそろえる
            }
        }
        phy_memory_manager.define_used_memory(memory_entry_pool_address, memory_entry_pool_size);
        /*カーネル領域の予約*/
        for section in multiboot_info.elf_info.clone() {
            phy_memory_manager.define_used_memory(section.addr(), section.size());
        }
        phy_memory_manager.define_used_memory(multiboot_info.address, multiboot_info.size);
        MemoryManager {
            physical_memory_manager: phy_memory_manager
        }
    }

    pub fn get_memory_pool(&self) -> (usize, usize) {
        self.physical_memory_manager.get_memory_entry_pool()
    }

    pub const fn new_static() -> MemoryManager {
        MemoryManager {
            physical_memory_manager: PhysicalMemoryManager::new(),
        }
    }

    pub fn alloc_page(&mut self, align: bool) -> Option<usize> {
        /*Test*/
        self.physical_memory_manager.alloc(PAGE_SIZE, align)
    }

    pub fn free_page(&mut self, address: usize) {
        self.physical_memory_manager.free(address, PAGE_SIZE);
    }

    pub fn dump_memory_manager(&self) {
        self.physical_memory_manager.dump_memory_entry();
    }
}
