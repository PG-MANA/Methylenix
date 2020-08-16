/*
 * PFF2 Font File
 * https://www.gnu.org/software/grub/manual/grub-dev/html_node/PFF2-Font-File-Format.html
 */

pub struct Pff2FontManager {
    base_address: usize,
    max_font_width: u16,
    max_font_height: u16,
    ascent: u16,
    decent: u16,
    char_index_address: usize,
    char_index_size: usize,
    data_address: usize,
    data_size: usize,
    font_point_size: u16,
    font_cache: FontCache,
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub struct FontData {
    pub width: u16,
    pub height: u16,
    pub x_offset: i16,
    pub y_offset: i16,
    pub device_width: i16,
    pub bitmap_address: usize,
}

pub struct FontCache {
    ascii: [FontData; 0x7f - 0x20],
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
    pub const fn new() -> Self {
        Self {
            base_address: 0,
            max_font_height: 0,
            max_font_width: 0,
            ascent: 0,
            decent: 0,
            char_index_address: 0,
            char_index_size: 0,
            data_address: 0,
            data_size: 0,
            font_point_size: 0,
            font_cache: FontCache::new(),
        }
    }

    pub fn load(&mut self, virtual_font_file_address: usize, size: usize) -> bool {
        /* Check the file structure */
        if unsafe { *(virtual_font_file_address as *const [u8; 12]) }
            != [
                0x46, 0x49, 0x4c, 0x45, 0x00, 0x00, 0x00, 0x04, 0x50, 0x46, 0x46, 0x32,
            ]
        /* FILE PFF2*/
        {
            return false;
        }
        self.base_address = virtual_font_file_address;

        let mut pointer = 12;

        while pointer < size {
            use core::{str, u16, u32};

            let section_type = str::from_utf8(unsafe {
                &*((virtual_font_file_address + pointer) as *const [u8; 4])
            })
            .unwrap_or("");
            let section_length = u32::from_be_bytes(unsafe {
                *((virtual_font_file_address + pointer + 4) as *const [u8; 4])
            }) as usize;
            pointer += 8;

            match section_type {
                "NAME" | "FAMI" | "WEIG" | "SLAN" => {}
                "PTSZ" => {
                    self.font_point_size = u16::from_be_bytes(unsafe {
                        *((virtual_font_file_address + pointer) as *const [u8; 2])
                    });
                }
                "MAXW" => {
                    self.max_font_width = u16::from_be_bytes(unsafe {
                        *((virtual_font_file_address + pointer) as *const [u8; 2])
                    });
                }
                "MAXH" => {
                    self.max_font_height = u16::from_be_bytes(unsafe {
                        *((virtual_font_file_address + pointer) as *const [u8; 2])
                    });
                }
                "ASCE" => {
                    self.ascent = u16::from_be_bytes(unsafe {
                        *((virtual_font_file_address + pointer) as *const [u8; 2])
                    });
                }
                "DESC" => {
                    self.decent = u16::from_be_bytes(unsafe {
                        *((virtual_font_file_address + pointer) as *const [u8; 2])
                    });
                }
                "CHIX" => {
                    self.char_index_address = virtual_font_file_address + pointer;
                    self.char_index_size = section_length;
                }
                "DATA" => {
                    self.data_address = virtual_font_file_address + pointer;
                    self.data_size = section_length;
                    break;
                }
                _ => {
                    return false;
                }
            };
            pointer += section_length;
        }
        self.build_ascii_cache();
        return true;
    }

    fn build_ascii_cache(&mut self) {
        use core::mem::size_of;
        let index_entry_size = size_of::<Pff2CharIndex>();
        let mut pointer = 0;
        for a in ' '..'\x7f' {
            let char_utf32 = [0, 0, 0, a as u8];
            let char_index = {
                let next_entry = unsafe {
                    &*((self.char_index_address + pointer * index_entry_size)
                        as *const Pff2CharIndex)
                };
                if next_entry.code == char_utf32 {
                    next_entry
                } else {
                    pointer = 0;
                    let mut entry;
                    loop {
                        entry = unsafe {
                            &*((self.char_index_address + pointer * index_entry_size)
                                as *const Pff2CharIndex)
                        };
                        if entry.code == char_utf32 {
                            break;
                        }
                        pointer += 1;
                        if pointer * index_entry_size >= self.char_index_size {
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
            let font_data = FontData {
                width: u16::from_be_bytes(pff2_font_data.width),
                height: u16::from_be_bytes(pff2_font_data.height),
                x_offset: i16::from_be_bytes(pff2_font_data.x_offset),
                y_offset: i16::from_be_bytes(pff2_font_data.y_offset),
                device_width: i16::from_be_bytes(pff2_font_data.device_width),
                bitmap_address: (&(pff2_font_data.bitmap) as *const u8 as usize),
            };

            self.font_cache.add_ascii_font_cache(a, font_data);
            pointer += 1;
        }
    }

    pub fn get_char_data(&mut self, c: char) -> Option<FontData> {
        if c.is_control() {
            return None;
        }
        if c.is_ascii() {
            Some(self.font_cache.get_cached_ascii_font_data(c))
        } else {
            None /*TODO*/
        }
    }
}

impl FontCache {
    const DEFAULT_CACHE_LEN: usize = 64;
    pub const fn new() -> Self {
        Self {
            ascii: [FontData::new_const(); 0x7f - 0x20],
        }
    }

    pub fn add_ascii_font_cache(&mut self, c: char, font_data: FontData) {
        assert!(c.is_ascii());
        self.ascii[(c as usize) - 0x20] = font_data;
    }

    pub fn get_cached_ascii_font_data(&self, c: char) -> FontData {
        assert!(c.is_ascii());
        self.ascii[(c as usize) - 0x20].clone()
    }
}

impl FontData {
    pub const fn new_const() -> Self {
        Self {
            width: 8,
            height: 16,
            device_width: 0,
            x_offset: 0,
            y_offset: 0,
            bitmap_address: 0,
        }
    }
}
