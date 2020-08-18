/*
 * Font Management
 * 現在はBitmapフォントでPff2だけだが、他の方式にも対応できるようにしている(trait作る必要あり)
 */

pub mod font_cache;
pub mod pff2;

#[derive(Copy, Clone, Eq, PartialEq)]
pub struct BitmapFontData {
    pub width: u16,
    pub height: u16,
    pub x_offset: i16,
    pub y_offset: i16,
    pub device_width: i16,
    pub bitmap_address: usize,
}

impl BitmapFontData {
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
