//!
//! Font Cache
//!
//! This module contains the cache of BitmapFontData.
//! All ascii data and DEFAULT_CACHE_LEN of non-ascii data are available.
//!

use super::BitmapFontData;

pub struct FontCache {
    ascii: [BitmapFontData; 0x7f - 0x20],
    normal: [(char, BitmapFontData); Self::DEFAULT_CACHE_LEN],
}

impl FontCache {
    const DEFAULT_CACHE_LEN: usize = 64;
    pub const fn new() -> Self {
        Self {
            ascii: [BitmapFontData::new_const(); 0x7f - 0x20],
            normal: [('\0', BitmapFontData::new_const()); Self::DEFAULT_CACHE_LEN],
        }
    }

    pub fn add_ascii_font_cache(&mut self, c: char, font_data: BitmapFontData) {
        assert!(c.is_ascii());
        self.ascii[(c as usize) - 0x20] = font_data;
    }

    pub fn get_cached_ascii_font_data(&self, c: char) -> BitmapFontData {
        assert!(c.is_ascii());
        self.ascii[(c as usize) - 0x20].clone()
    }

    pub fn add_normal_font_cache(&mut self, c: char, font_data: BitmapFontData) -> bool {
        if let Some(cache) = self.normal.iter_mut().find(|t| t.0 == '\0') {
            *cache = (c, font_data);
            true
        } else {
            false
        }
    }

    pub fn get_cached_normal_font_data(&self, c: char) -> Option<BitmapFontData> {
        if let Some(cache) = self.normal.iter().find(|&t| t.0 == c) {
            Some(cache.1.clone())
        } else {
            None
        }
    }
}
