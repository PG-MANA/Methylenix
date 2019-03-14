/*
メモリ管理システム
    とりあえず、MemManは4KBごとに空き、使用中か管理して詳しいことはページングでと考えてる
    メモリ領域がほしい(要求)=>まずあいているメモリをMemManで探しリアルアドレスをとってくる=>それを元にページを作成=>返す
    だといいかな。
    この実装は色んな本やサイトを参考にした書き方なのでいつか再実装する。
    というより土壇場実装なのでかなり危ない。
*/

use core::mem;
use kernel::drivers::multiboot::{MemoryMapEntry, MemoryMapInfo, MultiBootInformation};

pub struct Page {
    //本の1ページみたいな
    no_page: usize, //ページ番号
}

impl Page {
    pub const PAGE_SIZE: usize = 4 * 1024; //ページングにおける一ページのサイズに合わせる

    pub const fn make_from_address(addr: usize) -> Page {
        Page {
            no_page: addr / Page::PAGE_SIZE,
        }
    }

    pub fn get_page(&self) -> usize {
        self.no_page
    }
}

pub struct MemoryManager {
    current_area: &'static MemoryMapEntry, //現在いるMemoryMapでのメモリ領域
    current_next_page: Page, //現在使用されているページの次のページを指してる(head...?)
    memory_map: MemoryMapInfo,
    top_of_kernel_page: Page,
    bottom_of_kernel_page: Page,
    top_of_mbi_page: Page,
    bottom_of_mbi_page: Page,
}

impl MemoryManager {
    pub fn new(multiboot_info: &MultiBootInformation) -> MemoryManager {
        let kernel_loader_start = multiboot_info
            .elf_info
            .clone()
            .map(|section| section.addr())
            .min()
            .unwrap();
        let kernel_loader_end = multiboot_info
            .elf_info
            .clone()
            .map(|section| section.addr())
            .max()
            .unwrap();
        let mbi_start = multiboot_info.multiboot_information_address;
        let mbi_end = mbi_start + multiboot_info.multiboot_information_size as usize;
        let mut memory_manager = MemoryManager {
            current_area: unsafe { mem::uninitialized() },
            current_next_page: Page::make_from_address(0),
            memory_map: multiboot_info.memory_map_info.clone(),
            top_of_kernel_page: Page::make_from_address(kernel_loader_start),
            bottom_of_kernel_page: Page::make_from_address(kernel_loader_end),
            top_of_mbi_page: Page::make_from_address(mbi_start),
            bottom_of_mbi_page: Page::make_from_address(mbi_end),
        };
        memory_manager.select_next_area();
        memory_manager
    }

    pub const fn new_static() -> MemoryManager {
        MemoryManager {
            current_area: &MemoryMapEntry {
                addr: 0,
                length: 0,
                m_type: 0,
                reserved: 0,
            },
            current_next_page: Page::make_from_address(0),
            memory_map: MemoryMapInfo::new_static(),
            top_of_kernel_page: Page::make_from_address(0),
            bottom_of_kernel_page: Page::make_from_address(0),
            top_of_mbi_page: Page::make_from_address(0),
            bottom_of_mbi_page: Page::make_from_address(0),
        }
    }

    fn select_next_area(&mut self) {
        //あまりイケてない書き方であるが、色々filter使うより、ループ回すほうが早そう。
        //なおこのやり方は、memory_mapがアドレスから小さい方から並んでると勝手に信じて実装しているので、そうでなければアウト、filterとなんか使おうね。
        for memory_map_entry in self.memory_map.clone() {
            if memory_map_entry.m_type == 1
                && Page::make_from_address(
                    (memory_map_entry.addr + memory_map_entry.length - 1) as usize,
                )
                .no_page
                    >= self.current_next_page.no_page
            {
                self.current_area = memory_map_entry;
            }
        }
    }

    pub fn alloc_page(&mut self) -> Option<Page> {
        let current_area_last_frame = Page::make_from_address(
            (self.current_area.addr + self.current_area.length - 1) as usize,
        );

        if self.current_next_page.no_page > current_area_last_frame.no_page {
            //次の領域
            self.select_next_area();
        } else if self.current_next_page.no_page >= self.top_of_kernel_page.no_page
            && self.current_next_page.no_page <= self.bottom_of_kernel_page.no_page
        {
            //カーネル領域に入ったのでつまみ出す
            self.current_next_page.no_page = self.bottom_of_kernel_page.no_page + 1;
        } else if self.current_next_page.no_page >= self.top_of_mbi_page.no_page
            && self.current_next_page.no_page <= self.bottom_of_mbi_page.no_page
        {
            self.current_next_page.no_page = self.bottom_of_mbi_page.no_page + 1;
        } else {
            let cloned_page_next = Page {
                no_page: self.current_next_page.no_page,
            };
            self.current_next_page.no_page += 1;
            return Some(cloned_page_next);
        }
        self.alloc_page()
    }
}
