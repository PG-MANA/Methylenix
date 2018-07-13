/*
ページング実装(Page Table)
*/

use core::ops::{Index, IndexMut};
use paging::pte::PTE;

const PAGE_TABLE_MAX: usize = 512;

pub struct PageTable {
    entries: [PTE; PAGE_TABLE_MAX],
}

impl Index<usize> for PageTable {
    type Output = PTE;
    fn index(&self, i: usize) -> &PTE {
        &self.entries[i]
    }
}

impl IndexMut<usize> for PageTable {
    fn index_mut(&mut self, i: usize) -> &mut PTE {
        &mut self.entries[i]
    }
}

impl PageTable {
    pub fn init(&mut self) {
        for e in self.entries.iter_mut() {
            *e = PTE::new_empty();
        }
        /*let mut looper = PTE::new(self as *const _ as usize);
        looper.setPresent(true);
        looper.setWritable(true);
        self[PAGE_TABLE_MAX-1] =  looper;*/
    }
}
