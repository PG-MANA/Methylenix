//!
//! Font Manager
//!  
//! This manager handles font data.
//! Currently, this manage only PFF2 bitmap font data.
//!

pub mod font_cache;
pub mod pff2;

use pff2::Pff2FontManager;

use crate::kernel::memory_manager::data_type::{MSize, VAddress};

#[derive(Copy, Clone, Eq, PartialEq)]
pub struct BitmapFontData {
    pub width: u16,
    pub height: u16,
    pub x_offset: i16,
    pub y_offset: i16,
    pub device_width: i16,
    pub bitmap_address: VAddress,
}

impl Default for BitmapFontData {
    fn default() -> Self {
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
    manager: Pff2FontManager,
}

impl FontManager {
    pub fn new(font_address: VAddress, size: MSize, font_type: FontType) -> Option<Self> {
        match font_type {
            FontType::Pff2 => Some(Self {
                manager: Pff2FontManager::new(font_address, size)?,
            }),
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
