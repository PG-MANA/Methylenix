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

use arch::target_arch::device::vga_text::VgaTextDriver;

use kernel::drivers::multiboot::FrameBufferInfo;
use kernel::manager_cluster::get_kernel_manager_cluster;
use kernel::sync::spin_lock::{Mutex, SpinLockFlag};
use kernel::tty::Writer;

use core::fmt;

pub struct GraphicManager {
    lock: SpinLockFlag,
    text: Mutex<VgaTextDriver>,
    graphic: Mutex<FrameBufferManager>,
    is_text_mode: bool,
    font: FontManager,
}

impl GraphicManager {
    pub const fn new() -> Self {
        Self {
            lock: SpinLockFlag::new(),
            is_text_mode: false,
            text: Mutex::new(VgaTextDriver::new()),
            graphic: Mutex::new(FrameBufferManager::new()),
            font: FontManager::new(),
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

    pub fn init(&mut self, frame_buffer_info: &FrameBufferInfo) {
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
        virtual_font_address: usize,
        size: usize,
        font_type: FontType,
    ) -> bool {
        let _lock = self.lock.lock();
        self.font.load(virtual_font_address, size, font_type)
    }

    pub fn font_test(&mut self) {
        let _lock = self.lock.lock();
        let mut offset_x = 0usize;
        for c in "Methylenix, Rustで書かれたOSです。Grub2のunicode.pf2を解析しました。"
            .chars()
            .into_iter()
        {
            let a = self.font.get_font_data(c).unwrap();
            let font_bottom = self.font.get_ascent() as isize - a.y_offset as isize;
            let font_top = font_bottom as usize - a.height as usize;
            let font_left = (offset_x as isize + a.x_offset as isize) as usize;
            self.graphic.lock().unwrap().write_monochrome_bitmap(
                a.bitmap_address,
                a.width as usize,
                a.height as usize,
                font_left,
                font_top,
                0x55ffff,
                0,
                true,
            );
            offset_x = font_left + a.width as usize;
        }
    }

    pub fn puts(&self, string: &str) -> bool {
        get_kernel_manager_cluster()
            .serial_port_manager
            .sendstr(string);
        let _lock = self.lock.lock();
        if self.is_text_mode {
            self.text.lock().unwrap().puts(string)
        } else {
            true
        }
    }

    pub fn get_framer_buffer_size(&self) -> (usize /*x*/, usize /*y*/) {
        self.graphic.lock().unwrap().get_framer_buffer_size()
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
    fn write(&self, buf: &[u8], size_to_write: usize) -> fmt::Result {
        use core::str;
        if let Ok(s) = str::from_utf8(buf.split_at(size_to_write).0) {
            if self.puts(s) {
                //適当
                Ok(())
            } else {
                Err(fmt::Error {})
            }
        } else {
            Err(fmt::Error {})
        }
    }
}
