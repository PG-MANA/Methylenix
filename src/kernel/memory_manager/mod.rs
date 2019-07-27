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

pub struct MemoryManager {
    physical_memory_manager: PhysicalMemoryManager,
}

impl MemoryManager {
    pub fn new(multiboot_info: &MultiBootInformation) -> MemoryManager {
        let kernel_loader_range = (|multiboot_info: &MultiBootInformation| {
            let map = multiboot_info
                .elf_info
                .clone()
                .map(|section| section.addr());
            (map.clone().min().unwrap(), map.max().unwrap())
        })(multiboot_info);
        let mut memory_for_manager: (usize, bool) = (0, false);
        for entry in multiboot_info.memory_map_info.clone() {
            if memory_for_manager.1 == false && entry.m_type == 1 && entry.length as usize >= PAGE_SIZE &&
                !(entry.addr as usize + PAGE_SIZE - 1 >= kernel_loader_range.0 &&
                    entry.addr as usize + PAGE_SIZE - 1 <= kernel_loader_range.1) {
                /*Kernel Loaderはメモリマップに記載されてないので用意するメモリ領域の端がそれらに食い込まないかチェック*/
                memory_for_manager = (entry.addr as usize, true);
            }
        }
        let mut phy_memory_manager = PhysicalMemoryManager::new();
        phy_memory_manager.set_memory_entry_pool(memory_for_manager.0, PAGE_SIZE);
        for entry in multiboot_info.memory_map_info.clone() {
            /*2回回すのは少し気が引けるけど...*/
            if entry.m_type == 1 {
                phy_memory_manager.define_free_memory(entry.addr as usize, entry.length as usize);
            }
        }
        phy_memory_manager.define_used_memory(memory_for_manager.0, PAGE_SIZE);
        /*カーネル領域の予約*/
        for section in multiboot_info.elf_info.clone() {
            phy_memory_manager.define_used_memory(section.addr(), section.size());
        }
        /*phy_memory_manager.define_used_memory(multiboot_info.multiboot_information_address, multiboot_info.multiboot_information_size);
        マルチブートインフォメーションはすでにコピーされてるのでいらない。
        */
        MemoryManager {
            physical_memory_manager: phy_memory_manager
        }
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

    pub fn dump_memory_manager(&self) {
        self.physical_memory_manager.dump_memory_entry();
    }
}
