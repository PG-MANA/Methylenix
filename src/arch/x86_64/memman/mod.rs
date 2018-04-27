/*
 * Copyright 2017 PG_MANA
 *
 * This software is Licensed under the Apache License Version 2.0
 * See LICENSE.md
 *
 * メモリ管理システム
 * とりあえず、MemManは4KBごとに空き、使用中か管理して詳しいことはページングでと考えてる
 * 流れ)メモリ領域がほしい(要求)=>まずあいているメモリをMemManで探しリアルアドレスをとってくる=>それを元にページを作成=>返す
 * だといいかな
 * この実装は色んな本やサイトを参考にした書き方なのでいつか再実装する。
 * というより土壇場実装なのでかなり危ない
 */

use arch::x86_64::mbi::{MemoryMapEntry, MemoryMapInfo};
use core::mem;

pub struct Page {
    //本の1ページみたいな
    no_page: usize, //ページ番号
}

impl Page {
    pub const PAGE_SIZE: usize = 4 * 1024; //ページングにおける一ページのサイズに合わせる

    pub fn make_from_address(addr: usize) -> Page {
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
    pub fn new(
        memory_map: MemoryMapInfo,
        top_of_kernel_address: usize,
        bottom_of_kernel_address: usize,
        top_of_mbi_address: usize,
        bottom_of_mbi_address: usize,
    ) -> MemoryManager {
        let mut memman = MemoryManager {
            current_area: unsafe { mem::uninitialized() },
            current_next_page: Page::make_from_address(0),
            memory_map: memory_map,
            top_of_kernel_page: Page::make_from_address(top_of_kernel_address),
            bottom_of_kernel_page: Page::make_from_address(bottom_of_kernel_address),
            top_of_mbi_page: Page::make_from_address(top_of_mbi_address),
            bottom_of_mbi_page: Page::make_from_address(bottom_of_mbi_address),
        };
        memman.select_next_area();
        memman
    }

    fn select_next_area(&mut self) {
        //あまりイケてない書き方であるが、色々filter使うより、ループ回すほうが早そう。
        //なおこのやり方は、memory_mapがアドレスから小さい方から並んでると勝手に信じて実装しているので、そうでなければアウト、filterとなんか使おうね。
        for memory_map_entry in self.memory_map.clone() {
            if memory_map_entry.m_type == 1
                && Page::make_from_address(
                    (memory_map_entry.addr + memory_map_entry.length - 1) as usize,
                ).no_page >= self.current_next_page.no_page
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
