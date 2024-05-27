/*
 * Graphic Manager
 * いずれはstdio.rsみたいなのを作ってそれのサブモジュールにしたい
 */

pub mod font;
pub mod frame_buffer_manager;
pub mod text_buffer_driver;

use self::font::FontManager;
use self::font::FontType;
use self::frame_buffer_manager::FrameBufferManager;
use self::text_buffer_driver::TextBufferDriver;

use crate::arch::target_arch::device::text::TextDriver;

use crate::kernel::drivers::efi::protocol::graphics_output_protocol::EfiGraphicsOutputModeInformation;
use crate::kernel::drivers::multiboot::FrameBufferInfo;
use crate::kernel::memory_manager::data_type::{Address, VAddress};
use crate::kernel::sync::spin_lock::{Mutex, SpinLockFlag};
use crate::kernel::tty::Writer;

use core::fmt;

pub struct GraphicManager {
    lock: SpinLockFlag,
    text: Mutex<TextDriver>,
    graphic: Mutex<FrameBufferManager>,
    is_text_mode: bool,
    font: Mutex<FontManager>,
    cursor: Mutex<Cursor>,
    is_font_loaded: bool,
}

struct Cursor {
    x: usize,
    y: usize,
}

impl GraphicManager {
    pub const fn new() -> Self {
        Self {
            lock: SpinLockFlag::new(),
            is_text_mode: false,
            text: Mutex::new(TextDriver::new()),
            graphic: Mutex::new(FrameBufferManager::new()),
            font: Mutex::new(FontManager::new()),
            cursor: Mutex::new(Cursor { x: 0, y: 0 }),
            is_font_loaded: false,
        }
    }

    pub const fn is_text_mode(&self) -> bool {
        self.is_text_mode
    }

    pub fn set_frame_buffer_memory_permission(&mut self) -> bool {
        let _lock = self.lock.lock();
        if self.is_text_mode {
            self.text
                .lock()
                .unwrap()
                .set_frame_buffer_memory_permission()
        } else {
            self.graphic
                .lock()
                .unwrap()
                .set_frame_buffer_memory_permission()
        }
    }

    pub fn init_by_efi_information(
        &mut self,
        base_address: usize,
        memory_size: usize,
        pixel_info: &EfiGraphicsOutputModeInformation,
    ) {
        let _lock = self.lock.lock();
        self.graphic
            .lock()
            .unwrap()
            .init_by_efi_information(base_address, memory_size, pixel_info);
    }

    pub fn init_by_multiboot_information(&mut self, frame_buffer_info: &FrameBufferInfo) {
        let _lock = self.lock.lock();
        if !self
            .graphic
            .lock()
            .unwrap()
            .init_by_multiboot_information(frame_buffer_info)
        {
            self.text
                .lock()
                .unwrap()
                .init_by_multiboot_information(frame_buffer_info);
            self.is_text_mode = true;
        }
    }

    pub fn clear_screen(&mut self) {
        let _lock = self.lock.lock();
        if self.is_text_mode {
            self.text.lock().unwrap().clear_screen();
        } else {
            self.graphic.lock().unwrap().clear_screen();
        }
    }

    pub fn load_font(
        &mut self,
        virtual_font_address: VAddress,
        size: usize,
        font_type: FontType,
    ) -> bool {
        let _lock = self.lock.lock();
        self.is_font_loaded = self
            .font
            .lock()
            .unwrap()
            .load(virtual_font_address, size, font_type);
        self.is_font_loaded
    }

    fn draw_string(&self, s: &str, foreground_color: u32, background_color: u32) -> fmt::Result {
        /* assume locked */
        if !self.is_font_loaded {
            return Err(fmt::Error {});
        }
        let mut cursor = self.cursor.lock().unwrap();
        let mut font_manager = self.font.lock().unwrap();
        let mut frame_buffer_manager = self.graphic.lock().unwrap();
        let frame_buffer_size = frame_buffer_manager.get_frame_buffer_size();

        for c in s.chars() {
            if c == '\n' {
                cursor.x = 0;
                cursor.y += font_manager.get_max_font_height();
            } else if c == '\r' {
                cursor.x = 0;
            } else if c.is_control() {
            } else {
                let font_data = font_manager.get_font_data(c);
                if font_data.is_none() {
                    continue;
                }
                let font_data = font_data.unwrap();
                let font_bottom = font_manager.get_ascent() as isize - font_data.y_offset as isize;
                let font_top = font_bottom as usize - font_data.height as usize;
                let font_left = font_data.x_offset as usize;
                if frame_buffer_size.0 < cursor.x + font_data.width as usize {
                    cursor.x = 0;
                    cursor.y += font_manager.get_max_font_height();
                }
                if frame_buffer_size.1 < cursor.y + font_manager.get_max_font_height() {
                    let scroll_y =
                        font_manager.get_max_font_height() + cursor.y - frame_buffer_size.1;
                    frame_buffer_manager.scroll_screen(scroll_y);
                    frame_buffer_manager.fill(
                        0,
                        frame_buffer_size.1 - scroll_y,
                        frame_buffer_size.0,
                        frame_buffer_size.1,
                        0,
                    ); /* erase the last line */
                    cursor.y -= scroll_y;
                }

                frame_buffer_manager.write_monochrome_bitmap(
                    font_data.bitmap_address.to_usize(),
                    font_data.width as usize,
                    font_data.height as usize,
                    cursor.x + font_left,
                    cursor.y + font_top,
                    foreground_color,
                    background_color,
                    true,
                );
                cursor.x += font_data.device_width as usize;
            }
        }
        Ok(())
    }

    pub fn puts(&self, string: &str, foreground_color: u32, background_color: u32) -> bool {
        let _lock = if let Ok(l) = self.lock.try_lock() {
            l
        } else {
            return true;
        };
        if self.is_text_mode {
            self.text.lock().unwrap().puts(string)
        } else if self.is_font_loaded {
            self.draw_string(string, foreground_color, background_color)
                .is_ok()
        } else {
            true
        }
    }

    pub fn get_frame_buffer_size(&self) -> (usize /*x*/, usize /*y*/) {
        self.graphic.lock().unwrap().get_frame_buffer_size()
    }

    pub fn fill(&mut self, start_x: usize, start_y: usize, end_x: usize, end_y: usize, color: u32) {
        if !self.is_text_mode {
            let _lock = self.lock.lock();
            self.graphic
                .lock()
                .unwrap()
                .fill(start_x, start_y, end_x, end_y, color);
        }
    }

    pub fn write_bitmap(
        &mut self,
        buffer: usize,
        depth: u8,
        size_x: usize,
        size_y: usize,
        offset_x: usize,
        offset_y: usize,
    ) -> bool {
        if !self.is_text_mode {
            let _lock = self.lock.lock();
            self.graphic
                .lock()
                .unwrap()
                .write_bitmap(buffer, depth, size_x, size_y, offset_x, offset_y, false)
        } else {
            false
        }
    }
}

impl Writer for GraphicManager {
    fn write(
        &self,
        buf: &[u8],
        size_to_write: usize,
        foreground_color: u32,
        background_color: u32,
    ) -> fmt::Result {
        use core::str;
        if let Ok(s) = str::from_utf8(buf.split_at(size_to_write).0) {
            if self.puts(s, foreground_color, background_color) {
                Ok(())
            } else {
                Err(fmt::Error {})
            }
        } else {
            Err(fmt::Error {})
        }
    }
}
