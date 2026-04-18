//!
//! Graphic Manager
//!
//! This module handles writing string or bitmap to frame buffer
//!

pub mod font;

use self::font::{FontManager, FontType};

use crate::kernel::memory_manager::data_type::{
    Address, MSize, MemoryPermissionFlags, PAddress, VAddress,
};
use crate::kernel::memory_manager::{MemoryError, io_remap};
use crate::kernel::sync::spin_lock::Mutex;
use crate::kernel::tty::{TextColor, Writer};

use core::fmt;
use core::ptr::{copy, read_unaligned, read_volatile, write_unaligned, write_volatile};

#[derive(Default, Copy, Clone, Debug)]
struct Point {
    x: usize,
    y: usize,
}

impl Point {
    const fn new(x: usize, y: usize) -> Self {
        Self { x, y }
    }
}

#[derive(Default)]
struct FrameBuffer {
    address: usize,
    size: Point,
    color_depth: u8,
}

pub struct GraphicManager {
    frame_buffer: Mutex<FrameBuffer>,
    font: Mutex<Option<FontManager>>,
    text_cursor: Mutex<Point>,
}

impl Default for GraphicManager {
    fn default() -> Self {
        Self {
            frame_buffer: Mutex::new(FrameBuffer::default()),
            font: Mutex::new(None),
            text_cursor: Mutex::new(Point::default()),
        }
    }
}

impl GraphicManager {
    pub fn set_frame_buffer(
        &mut self,
        address: PAddress,
        width: usize,
        height: usize,
        color_depth: u8,
    ) {
        *self.frame_buffer.lock().unwrap() = FrameBuffer {
            address: address.to_usize(),
            size: Point::new(width, height),
            color_depth,
        };
    }

    pub fn remap_frame_buffer(&mut self) -> Result<(), MemoryError> {
        let mut frame_buffer = self.frame_buffer.lock().unwrap();
        io_remap!(
            PAddress::new(frame_buffer.address),
            MSize::new(
                frame_buffer.size.x
                    * frame_buffer.size.y
                    * (frame_buffer.color_depth >> 3) as usize,
            ),
            MemoryPermissionFlags::data()
        )
        .map(|a| frame_buffer.address = a.to_usize())
    }

    pub fn load_font(&mut self, font_address: VAddress, size: MSize, font_type: FontType) -> bool {
        let Some(font_manager) = FontManager::new(font_address, size, font_type) else {
            return false;
        };
        /* TODO: treat old font */
        self.font.lock().unwrap().replace(font_manager);
        true
    }

    /* Graphic section */
    fn _fill(frame_buffer: &FrameBuffer, start: Point, end: Point, color: u32) {
        assert!(start.x < end.x);
        assert!(start.y < end.y);
        assert!(end.x <= frame_buffer.size.x);
        assert!(end.y <= frame_buffer.size.y);

        if frame_buffer.color_depth == 32 {
            for y in start.y..end.y {
                for x in start.x..end.x {
                    unsafe {
                        write_volatile(
                            (frame_buffer.address + ((y * frame_buffer.size.x + x) * 4))
                                as *mut u32,
                            color,
                        )
                    };
                }
            }
        } else if frame_buffer.color_depth == 24 {
            for y in start.y..end.y {
                for x in start.x..end.x {
                    let dot =
                        (frame_buffer.address + (y * frame_buffer.size.x + x) * 3) as *mut u32;
                    let mut current = unsafe { read_unaligned(dot) };
                    current &= 0x000000ff;
                    current |= color;
                    unsafe { write_unaligned(dot, current) };
                }
            }
        }
    }

    fn _scroll(frame_buffer: &FrameBuffer, from: Point, to: Point, size: Point) {
        assert!(from.x + size.x <= frame_buffer.size.x);
        assert!(from.y + size.y <= frame_buffer.size.y);
        assert!(to.x <= from.x);
        assert!(to.y <= from.y);
        if frame_buffer.color_depth == 32 {
            for y in 0..size.y {
                unsafe {
                    copy(
                        (frame_buffer.address + ((from.y + y) * frame_buffer.size.x + from.x) * 4)
                            as *mut u32,
                        (frame_buffer.address + ((to.y + y) * frame_buffer.size.x + to.x) * 4)
                            as *mut u32,
                        size.x,
                    )
                };
            }
        } else if frame_buffer.color_depth == 24 {
            for y in 0..size.y {
                unsafe {
                    copy(
                        (frame_buffer.address + ((from.y + y) * frame_buffer.size.x + from.x) * 3)
                            as *mut u8,
                        (frame_buffer.address + ((to.y + y) * frame_buffer.size.x + to.x) * 3)
                            as *mut u8,
                        size.x * 3,
                    )
                };
            }
        }
    }

    fn _scroll_screen(frame_buffer: &FrameBuffer, height: usize) {
        assert!(height < frame_buffer.size.y);
        let color_depth_byte = (frame_buffer.color_depth >> 3) as usize;
        let mut src = frame_buffer.address + height * frame_buffer.size.x * color_depth_byte;
        let mut dst = frame_buffer.address;
        let end = frame_buffer.address
            + (frame_buffer.size.y - height) * frame_buffer.size.x * color_depth_byte;
        let quad_word_copy_end = if (end & 7) == 0 { end - 8 } else { end & !7 };

        while dst < quad_word_copy_end {
            unsafe { write_volatile(dst as *mut u64, read_volatile(src as *const u64)) };
            src += 1 << 3;
            dst += 1 << 3;
        }
        while dst < end {
            unsafe { write_volatile(dst as *mut u8, read_volatile(src as *const u8)) };
            src += 1 << 3;
            src += 1;
            dst += 1;
        }
    }

    fn _write_monochrome_bitmap(
        frame_buffer: &FrameBuffer,
        buffer: usize,
        size: Point,
        offset: Point,
        front_color: u32,
        back_color: u32,
        is_not_aligned_data: bool,
    ) {
        assert!(frame_buffer.color_depth == 32 || frame_buffer.color_depth == 24);
        let screen_depth_byte = frame_buffer.color_depth as usize >> 3;
        let bitmap_padding = if is_not_aligned_data { 0 } else { size.x & 7 };
        let mut bitmap_pointer = buffer;
        let mut bitmap_mask = 0x80;
        let mut buffer_pointer =
            frame_buffer.address + (offset.y * frame_buffer.size.x + offset.x) * screen_depth_byte;

        if frame_buffer.color_depth == 32 {
            for _ in 0..size.y {
                for _ in 0..size.x {
                    unsafe {
                        write_volatile(
                            buffer_pointer as *mut u32,
                            if (*(bitmap_pointer as *const u8) & bitmap_mask) != 0 {
                                front_color
                            } else {
                                back_color
                            },
                        )
                    };
                    buffer_pointer += screen_depth_byte;
                    bitmap_mask >>= 1;
                    if bitmap_mask == 0 {
                        bitmap_pointer += 1;
                        bitmap_mask = 0x80;
                    }
                }
                buffer_pointer += (frame_buffer.size.x - size.x) * screen_depth_byte;
                if !is_not_aligned_data {
                    bitmap_pointer += bitmap_padding;
                    bitmap_mask = 0x80;
                }
            }
        } else {
            for _ in 0..size.y {
                for _ in 0..size.x {
                    let dot = buffer_pointer as *mut u32;
                    let mut current;
                    unsafe {
                        current = read_unaligned(dot);
                        current &= 0x000000ff;
                        if (*(bitmap_pointer as *const u8) & bitmap_mask) != 0 {
                            current |= front_color
                        } else {
                            current |= back_color
                        };
                        write_unaligned(dot, current & 0xffffff);
                    }
                    buffer_pointer += screen_depth_byte;
                    bitmap_mask >>= 1;
                    if bitmap_mask == 0 {
                        bitmap_pointer += 1;
                        bitmap_mask = 0x80;
                    }
                }
                buffer_pointer += (frame_buffer.size.x - size.x) * screen_depth_byte;
                if !is_not_aligned_data {
                    bitmap_pointer += bitmap_padding;
                    bitmap_mask = 0x80;
                }
            }
        }
    }

    fn _write_bitmap(
        frame_buffer: &FrameBuffer,
        buffer: usize,
        depth: u8,
        size: Point,
        offset: Point,
        is_not_aligned_data: bool,
    ) {
        assert!(frame_buffer.color_depth == 32 || frame_buffer.color_depth == 24);
        let screen_depth_byte = frame_buffer.color_depth as usize / 8;
        let bitmap_depth_byte = depth as usize / 8;
        let bitmap_aligned_bitmap_width_pointer = if is_not_aligned_data {
            size.x
        } else {
            ((size.x * bitmap_depth_byte - 1) & !3) + 4
        };

        if frame_buffer.color_depth == 32 {
            for height_pointer in (0..size.y).rev() {
                for width_pointer in 0..size.x {
                    unsafe {
                        write_volatile(
                            (frame_buffer.address
                                + ((height_pointer + offset.y) * frame_buffer.size.x
                                    + offset.x
                                    + width_pointer)
                                    * screen_depth_byte) as *mut u32,
                            read_unaligned(
                                (buffer
                                    + (size.y - height_pointer - 1)
                                        * bitmap_aligned_bitmap_width_pointer
                                    + width_pointer * bitmap_depth_byte)
                                    as *const u32,
                            ),
                        );
                    }
                }
            }
        } else {
            for height_pointer in (0..size.y).rev() {
                for width_pointer in 0..size.x {
                    let dot = (frame_buffer.address
                        + ((height_pointer + offset.y) * frame_buffer.size.x
                            + offset.x
                            + width_pointer)
                            * screen_depth_byte) as *mut u32;
                    let mut current;
                    unsafe {
                        current = read_unaligned(dot);
                        current &= 0x000000ff;
                        current |= read_unaligned(
                            (buffer
                                + (size.y - height_pointer) * bitmap_aligned_bitmap_width_pointer
                                + width_pointer * bitmap_depth_byte)
                                as *const u32,
                        ) & 0xffffff;
                        write_unaligned(dot, current);
                    }
                }
            }
        }
    }

    pub fn clear_screen(&self) {
        let frame_buffer = self.frame_buffer.lock().unwrap();
        Self::_fill(
            &frame_buffer,
            Point::new(0, 0),
            Point::new(frame_buffer.size.x, frame_buffer.size.y),
            0,
        );
    }

    /* string rendering section */

    fn draw_string(&self, s: &str, foreground_color: u32, background_color: u32) -> fmt::Result {
        let mut font_manager = self.font.lock().unwrap();
        let Some(font_manager) = font_manager.as_mut() else {
            return Ok(());
        };
        let mut cursor = self.text_cursor.lock().unwrap();
        let framer_buffer = self.frame_buffer.lock().unwrap();

        for c in s.chars() {
            if c == '\n' {
                cursor.x = 0;
                cursor.y += font_manager.get_max_font_height();
            } else if c == '\r' {
                cursor.x = 0;
            } else if c.is_control() {
                /* Ignore */
            } else {
                let font_data = font_manager.get_font_data(c);
                if font_data.is_none() {
                    continue;
                }
                let font_data = font_data.unwrap();
                let font_bottom = font_manager.get_ascent() as isize - font_data.y_offset as isize;
                let font_top = font_bottom as usize - font_data.height as usize;
                let font_left = font_data.x_offset as usize;
                if framer_buffer.size.x < cursor.x + font_data.width as usize {
                    cursor.x = 0;
                    cursor.y += font_manager.get_max_font_height();
                }
                if framer_buffer.size.y < cursor.y + font_manager.get_max_font_height() {
                    let scroll_y =
                        font_manager.get_max_font_height() + cursor.y - framer_buffer.size.y;
                    Self::_scroll_screen(&framer_buffer, scroll_y);
                    Self::_fill(
                        &framer_buffer,
                        Point::new(0, framer_buffer.size.y - scroll_y),
                        Point::new(framer_buffer.size.x, framer_buffer.size.y),
                        0,
                    ); /* erase the last line */
                    cursor.y -= scroll_y;
                }

                Self::_write_monochrome_bitmap(
                    &framer_buffer,
                    font_data.bitmap_address.to_usize(),
                    Point::new(font_data.width as usize, font_data.height as usize),
                    Point::new(cursor.x + font_left, cursor.y + font_top),
                    foreground_color,
                    background_color,
                    true,
                );
                cursor.x += font_data.device_width as usize;
            }
        }
        Ok(())
    }

    pub fn get_frame_buffer_size(&self) -> (usize /*x*/, usize /*y*/) {
        let f = self.frame_buffer.lock().unwrap();
        (f.size.x, f.size.y)
    }

    pub fn write_bitmap(
        &mut self,
        buffer: usize,
        depth: u8,
        size_x: usize,
        size_y: usize,
        offset_x: usize,
        offset_y: usize,
    ) {
        Self::_write_bitmap(
            &self.frame_buffer.lock().unwrap(),
            buffer,
            depth,
            Point::new(size_x, size_y),
            Point::new(offset_x, offset_y),
            false, /*TODO: consider*/
        )
    }
}

impl Writer for GraphicManager {
    fn write(&self, buf: &[u8], foreground: TextColor, background: TextColor) -> fmt::Result {
        let Ok(s) = str::from_utf8(buf) else {
            return Err(fmt::Error);
        };
        self.draw_string(s, foreground.to_u32(), background.to_u32())
    }
}
