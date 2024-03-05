//!
//! PFF2 Font Manager
//!
//! This manager handles PFF2 Font data.
//! <https://www.gnu.org/software/grub/manual/grub-dev/html_node/PFF2-Font-File-Format.html>

use super::font_cache::FontCache;
use super::BitmapFontData;

use crate::kernel::memory_manager::data_type::{Address, VAddress};

pub struct Pff2FontManager {
    base_address: usize,
    /* max_font_width: u16, */
    max_font_height: u16,
    ascent: u16,
    decent: u16,
    char_index_address: usize,
    char_index_size: usize,
    /* data_address: usize, */
    /* data_size: usize, */
    /* font_point_size: u16, */
    font_cache: FontCache,
}

#[repr(C, packed)]
struct Pff2CharIndex {
    code: [u8; 4],
    flags: u8,
    offset: [u8; 4],
}

#[repr(C, packed)]
struct Pff2FontData {
    width: [u8; 2],
    height: [u8; 2],
    x_offset: [u8; 2],
    y_offset: [u8; 2],
    device_width: [u8; 2],
    bitmap: u8,
}

impl Pff2FontManager {
    const CHAR_INDEX_SIZE: usize = core::mem::size_of::<Pff2CharIndex>();

    pub const fn new() -> Self {
        Self {
            base_address: 0,
            max_font_height: 0,
            /* max_font_width: 0, */
            ascent: 0,
            decent: 0,
            char_index_address: 0,
            char_index_size: 0,
            /* data_address: 0, */
            /* data_size: 0, */
            /* font_point_size: 0, */
            font_cache: FontCache::new(),
        }
    }

    pub fn load(&mut self, virtual_font_file_address: VAddress, size: usize) -> bool {
        /* Check the file structure */
        if unsafe { *(virtual_font_file_address.to_usize() as *const [u8; 12]) }
            != [
                0x46, 0x49, 0x4c, 0x45, 0x00, 0x00, 0x00, 0x04, 0x50, 0x46, 0x46, 0x32,
            ]
        /* FILE PFF2*/
        {
            return false;
        }
        self.base_address = virtual_font_file_address.to_usize();

        let mut pointer = 12;

        while pointer < size {
            use core::{str, u16, u32};

            let section_type =
                str::from_utf8(unsafe { &*((self.base_address + pointer) as *const [u8; 4]) })
                    .unwrap_or("");
            let section_length = u32::from_be_bytes(unsafe {
                *((self.base_address + pointer + 4) as *const [u8; 4])
            }) as usize;
            pointer += 8;

            match section_type {
                "NAME" | "FAMI" | "WEIG" | "SLAN" => {}
                "PTSZ" => {
                    /* self.font_point_size = u16::from_be_bytes(unsafe {
                        *((self.base_address + pointer) as *const [u8; 2])
                    }); */
                }
                "MAXW" => {
                    /* self.max_font_width = u16::from_be_bytes(unsafe {
                        *((self.base_address + pointer) as *const [u8; 2])
                    }); */
                }
                "MAXH" => {
                    self.max_font_height = u16::from_be_bytes(unsafe {
                        *((self.base_address + pointer) as *const [u8; 2])
                    });
                }
                "ASCE" => {
                    self.ascent = u16::from_be_bytes(unsafe {
                        *((self.base_address + pointer) as *const [u8; 2])
                    });
                }
                "DESC" => {
                    self.decent = u16::from_be_bytes(unsafe {
                        *((self.base_address + pointer) as *const [u8; 2])
                    });
                }
                "CHIX" => {
                    self.char_index_address = self.base_address + pointer;
                    self.char_index_size = section_length;
                }
                "DATA" => {
                    /* self.data_address = self.base_address + pointer; */
                    /* self.data_size = section_length; */
                    break;
                }
                _ => {
                    return false;
                }
            };
            pointer += section_length;
        }
        self.build_ascii_cache();
        true
    }

    fn build_ascii_cache(&mut self) {
        let mut pointer = self.char_index_address;

        for a in ' '..'\x7f' {
            let char_utf32 = [0, 0, 0, a as u8];
            let char_index = {
                let next_entry = unsafe { &*(pointer as *const Pff2CharIndex) };
                if next_entry.code == char_utf32 {
                    next_entry
                } else {
                    pointer = self.char_index_address;
                    let limit = self.char_index_address + self.char_index_size;
                    let mut entry;

                    loop {
                        entry = unsafe { &*(pointer as *const Pff2CharIndex) };
                        if entry.code == char_utf32 {
                            break;
                        }
                        pointer += Self::CHAR_INDEX_SIZE;
                        if pointer >= limit {
                            return;
                        }
                    }
                    entry
                }
            };

            let pff2_font_data = unsafe {
                &*((u32::from_be_bytes(char_index.offset) as usize + self.base_address)
                    as *const Pff2FontData)
            };
            let font_data = Self::pff2_font_data_to_font_data(pff2_font_data);

            self.font_cache.add_ascii_font_cache(a, font_data);
            pointer += Self::CHAR_INDEX_SIZE;
        }
    }

    fn pff2_font_data_to_font_data(pff2_font_data: &Pff2FontData) -> BitmapFontData {
        BitmapFontData {
            width: u16::from_be_bytes(pff2_font_data.width),
            height: u16::from_be_bytes(pff2_font_data.height),
            x_offset: i16::from_be_bytes(pff2_font_data.x_offset),
            y_offset: i16::from_be_bytes(pff2_font_data.y_offset),
            device_width: i16::from_be_bytes(pff2_font_data.device_width),
            bitmap_address: VAddress::new(&(pff2_font_data.bitmap) as *const u8 as usize),
        }
    }

    pub const fn get_ascent(&self) -> u16 {
        self.ascent
    }

    pub const fn get_decent(&self) -> u16 {
        self.decent
    }

    pub const fn get_max_font_height(&self) -> u16 {
        self.max_font_height
    }

    pub fn get_char_font_data(&mut self, c: char) -> Option<BitmapFontData> {
        if c.is_control() {
            None
        } else if c.is_ascii() {
            Some(self.font_cache.get_cached_ascii_font_data(c))
        } else if let Some(f) = self.font_cache.get_cached_normal_font_data(c) {
            Some(f)
        } else if let Some(f) = self.find_uni_code_data(c) {
            self.font_cache.add_normal_font_cache(c, f);
            Some(f)
        } else {
            None
        }
    }

    fn find_uni_code_data(&self, c: char) -> Option<BitmapFontData> {
        let char_utf32: [u8; 4] = (c as u32).to_be_bytes();
        let char_index = {
            let mut entry;
            let mut pointer = self.char_index_address;
            let limit = self.char_index_address + self.char_index_size;

            loop {
                entry = unsafe { &*(pointer as *const Pff2CharIndex) };
                if entry.code == char_utf32 {
                    break;
                }
                pointer += Self::CHAR_INDEX_SIZE;
                if pointer >= limit {
                    return None;
                }
            }
            entry
        };
        let pff2_font_data = unsafe {
            &*((u32::from_be_bytes(char_index.offset) as usize + self.base_address)
                as *const Pff2FontData)
        };
        Some(Self::pff2_font_data_to_font_data(pff2_font_data))
    }
}
