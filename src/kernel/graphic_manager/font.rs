/*
 * Font Management
 * 現在はBitmapフォントでPff2だけだが、他の方式にも対応できるようにしている(trait作る必要あり)
 */

pub mod font_cache;
pub mod pff2;

use self::pff2::Pff2FontManager;

use crate::kernel::memory_manager::data_type::VAddress;

#[derive(Copy, Clone, Eq, PartialEq)]
pub struct BitmapFontData {
    pub width: u16,
    pub height: u16,
    pub x_offset: i16,
    pub y_offset: i16,
    pub device_width: i16,
    pub bitmap_address: VAddress,
}

impl BitmapFontData {
    pub const fn new_const() -> Self {
        Self {
            width: 8,
            height: 16,
            device_width: 0,
            x_offset: 0,
            y_offset: 0,
            bitmap_address: VAddress::new(0),
        }
    }
}

pub enum FontType {
    Pff2,
}

pub struct FontManager {
    manager: Pff2FontManager, /*これしかないので*/
}

impl FontManager {
    pub const fn new() -> Self {
        Self {
            manager: Pff2FontManager::new(),
        }
    }

    pub fn load(
        &mut self,
        virtual_font_address: VAddress,
        size: usize,
        font_type: FontType,
    ) -> bool {
        match font_type {
            FontType::Pff2 => self.manager.load(virtual_font_address, size),
        }
    }

    pub fn get_font_data(&mut self, c: char) -> Option<BitmapFontData> {
        self.manager.get_char_font_data(c)
    }

    pub fn get_ascent(&self) -> usize {
        self.manager.get_ascent() as usize
    }

    pub fn get_decent(&self) -> usize {
        self.manager.get_decent() as usize
    }

    pub fn get_max_font_height(&self) -> usize {
        self.manager.get_max_font_height() as usize
    }
}
