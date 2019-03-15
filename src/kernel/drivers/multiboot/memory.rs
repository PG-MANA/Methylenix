/*
MultiBoot2実装(メモリ関係)
*/

use core::mem;

#[derive(Clone)]
#[repr(C)]
pub struct MemoryMapEntry {
    pub addr: u64,
    pub length: u64,
    pub m_type: u32,
    pub reserved: u32,
}

#[repr(packed)]
#[allow(dead_code)]
pub struct MultibootTagMemoryMap {
    s_type: u32,
    size: u32,
    entry_size: u32,
    entry_version: u32,
    entries: MemoryMapEntry,
}

#[derive(Clone)]
pub struct MemoryMapInfo {
    pub addr: usize,
    pub num: u32,
    pub entry_size: u32,
    cnt: u32,
}

impl MemoryMapInfo {
    pub fn new(map: &MultibootTagMemoryMap) -> MemoryMapInfo {
        MemoryMapInfo {
            num: ((map.size as usize - mem::size_of::<MultibootTagMemoryMap>())
                / map.entry_size as usize) as u32, /*+1,//0からカウントするため-1するが打ち消し*/
            addr: unsafe { &map.entries as *const MemoryMapEntry as usize },
            entry_size: map.entry_size,
            cnt: 0,
        }
    }

    pub const fn new_static() -> MemoryMapInfo {
        MemoryMapInfo {
            addr: 0,
            num: 0,
            entry_size: 0,
            cnt: 0,
        }
    }

    pub fn reset(&mut self) {
        self.cnt = 0;
    }
}

impl Iterator for MemoryMapInfo {
    type Item = &'static MemoryMapEntry;
    //                            ↓の'はライフタイムと呼ばれ、返したあとにElf_Sectionが消えないようにするため。
    fn next(&mut self) -> Option<&'static MemoryMapEntry> {
        //これの実装でfor ... inが使える https://rustbyexample.com/trait/iter.html
        if self.cnt == self.num {
            return None;
        }
        let entry = unsafe {
            &*((self.addr + (self.cnt * self.entry_size) as usize) as *const MemoryMapEntry)
        };
        self.cnt += 1;
        return Some(entry);
    }
}

impl Default for MemoryMapInfo {
    fn default() -> MemoryMapInfo {
        MemoryMapInfo {
            addr: 0,
            num: 0,
            entry_size: 0,
            cnt: 0,
        }
    }
}

#[repr(C)]
pub struct MultibootTagBasicMeminfo {
    s_type: u32,
    size: u32,
    mem_lower: u32, //0MiB~からの空きメモリ
    mem_upper: u32, //1MiB~からの空きメモリ
}

pub struct MemoryInfo {
    pub mem_lower: u32, //0MiB~からの空きメモリ
    pub mem_upper: u32, //1MiB~からの空きメモリ
}

impl MemoryInfo {
    pub fn new(mem: &MultibootTagBasicMeminfo) -> MemoryInfo {
        MemoryInfo {
            mem_upper: mem.mem_upper,
            mem_lower: mem.mem_lower,
        }
    }
}

impl Default for MemoryInfo {
    fn default() -> MemoryInfo {
        MemoryInfo {
            mem_upper: 0,
            mem_lower: 0,
        }
    }
}