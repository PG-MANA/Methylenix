//!
//! PFF2 Font Manager
//!
//! This manager handles PFF2 Font data.
//! <https://www.gnu.org/software/grub/manual/grub-dev/html_node/PFF2-Font-File-Format.html>

use super::BitmapFontData;
use super::font_cache::FontCache;

use crate::kernel::memory_manager::data_type::{Address, MSize, VAddress};

pub struct Pff2FontManager {
    base_address: VAddress,
    /* max_font_width: u16, */
    max_font_height: u16,
    ascent: u16,
    decent: u16,
    char_index_address: VAddress,
    char_index_size: MSize,
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
    const SIGNATURE: [u8; 12] = [
        0x46, 0x49, 0x4c, 0x45, 0x00, 0x00, 0x00, 0x04, 0x50, 0x46, 0x46, 0x32,
    ];
    const CHAR_INDEX_SIZE: MSize = MSize::new(size_of::<Pff2CharIndex>());

    pub fn new(base_address: VAddress, size: MSize) -> Option<Self> {
        /* Check the file structure */
        if unsafe { *(base_address.to_usize() as *const [u8; 12]) } != Self::SIGNATURE {
            return None;
        }

        let mut pointer = MSize::new(12);
        let mut max_font_height = None;
        let mut ascent = None;
        let mut decent = None;
        let mut char_index_address = None;
        let mut char_index_size = None;

        while pointer < size {
            let section_type =
                str::from_utf8(unsafe { &*((base_address + pointer).to::<[u8; 4]>()) })
                    .unwrap_or("");
            pointer += MSize::new(4);
            let section_length = MSize::new(u32::from_be_bytes(unsafe {
                *((base_address + pointer).to::<[u8; 4]>())
            }) as usize);
            pointer += MSize::new(4);

            match section_type {
                "NAME" | "FAMI" | "WEIG" | "SLAN" | "PTSZ" | "MAXW" => {}
                "MAXH" => {
                    max_font_height = Some(u16::from_be_bytes(unsafe {
                        *((base_address + pointer).to::<[u8; 2]>())
                    }));
                }
                "ASCE" => {
                    ascent = Some(u16::from_be_bytes(unsafe {
                        *((base_address + pointer).to::<[u8; 2]>())
                    }));
                }
                "DESC" => {
                    decent = Some(u16::from_be_bytes(unsafe {
                        *((base_address + pointer).to::<[u8; 2]>())
                    }));
                }
                "CHIX" => {
                    char_index_address = Some(base_address + pointer);
                    char_index_size = Some(section_length);
                }
                "DATA" => {
                    break;
                }
                _ => {
                    return None;
                }
            };
            pointer += section_length;
        }
        let mut s = Self {
            base_address,
            max_font_height: max_font_height?,
            ascent: ascent?,
            decent: decent?,
            char_index_address: char_index_address?,
            char_index_size: char_index_size?,
            font_cache: FontCache::default(),
        };
        s.build_ascii_cache();
        Some(s)
    }

    fn build_ascii_cache(&mut self) {
        let mut pointer = self.char_index_address;

        for a in ' '..'\x7f' {
            let char_utf32 = [0, 0, 0, a as u8];
            let char_index = {
                let next_entry = unsafe { &*(pointer.to::<Pff2CharIndex>()) };
                if next_entry.code == char_utf32 {
                    next_entry
                } else {
                    pointer = self.char_index_address;
                    let limit = self.char_index_address + self.char_index_size;
                    let mut entry;

                    loop {
                        entry = unsafe { &*(pointer.to::<Pff2CharIndex>()) };
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
                &*((self.base_address + MSize::new(u32::from_be_bytes(char_index.offset) as usize))
                    .to::<Pff2FontData>())
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
            bitmap_address: VAddress::from(&(pff2_font_data.bitmap) as *const u8),
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
                entry = unsafe { &*(pointer.to::<Pff2CharIndex>()) };
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
            &*((self.base_address + MSize::new(u32::from_be_bytes(char_index.offset) as usize))
                .to::<Pff2FontData>())
        };
        Some(Self::pff2_font_data_to_font_data(pff2_font_data))
    }
}
