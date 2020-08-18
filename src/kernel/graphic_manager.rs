/*
 * Graphic Manager
 * いずれはstdio.rsみたいなのを作ってそれのサブモジュールにしたい
 */

pub mod font;
pub mod frame_buffer_manager;
pub mod text_buffer_driver;

use self::font::pff2::Pff2FontManager;
use self::frame_buffer_manager::FrameBufferManager;
use self::text_buffer_driver::TextBufferDriver;

use arch::target_arch::device::vga_text::VgaTextDriver;

use kernel::drivers::multiboot::FrameBufferInfo;
use kernel::manager_cluster::get_kernel_manager_cluster;

use core::fmt;

pub struct GraphicManager {
    text: VgaTextDriver,
    graphic: FrameBufferManager,
    is_text_mode: bool,
    font: Pff2FontManager,
}

impl GraphicManager {
    pub const fn new() -> Self {
        Self {
            is_text_mode: false,
            text: VgaTextDriver::new(),
            graphic: FrameBufferManager::new(),
            font: Pff2FontManager::new(),
        }
    }

    pub const fn is_text_mode(&self) -> bool {
        self.is_text_mode
    }

    pub fn set_frame_buffer_memory_permission(&mut self) -> bool {
        if self.is_text_mode {
            self.text.set_frame_buffer_memory_permission()
        } else {
            self.graphic.set_frame_buffer_memory_permission()
        }
    }

    pub fn init(&mut self, frame_buffer_info: &FrameBufferInfo) {
        if !self
            .graphic
            .init_by_multiboot_information(frame_buffer_info)
        {
            self.text.init_by_multiboot_information(frame_buffer_info);
            self.is_text_mode = true;
        }
    }

    pub fn clear_screen(&mut self) {
        if self.is_text_mode {
            self.text.clear_screen();
        } else {
            self.graphic.clear_screen();
        }
    }

    pub fn load_pff2_font(&mut self, virtual_font_address: usize, size: usize) -> bool {
        self.font.load(virtual_font_address, size)
    }

    pub fn font_test(&mut self) {
        let mut offset_x = 0usize;
        for c in "Methylenix, Rustで書かれたOSです。Grub2のunicode.pf2を解析しました。"
            .chars()
            .into_iter()
        {
            let a = self.font.get_char_font_data(c).unwrap();
            let font_bottom = self.font.get_ascent() as isize - a.y_offset as isize;
            let font_top = font_bottom as usize - a.height as usize;
            let font_left = (offset_x as isize + a.x_offset as isize) as usize;
            self.graphic.write_monochrome_bitmap(
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

    pub fn puts(&mut self, string: &str) -> bool {
        get_kernel_manager_cluster()
            .serial_port_manager
            .sendstr(string);
        if self.is_text_mode {
            self.text.puts(string)
        } else {
            true
        }
    }

    pub const fn get_framer_buffer_size(&self) -> (usize /*x*/, usize /*y*/) {
        self.graphic.get_framer_buffer_size()
    }

    pub fn fill(&mut self, start_x: usize, start_y: usize, end_x: usize, end_y: usize, color: u32) {
        if !self.is_text_mode {
            self.graphic.fill(start_x, start_y, end_x, end_y, color);
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
            self.graphic
                .write_bitmap(buffer, depth, size_x, size_y, offset_x, offset_y, false)
        } else {
            false
        }
    }
}

impl fmt::Write for GraphicManager {
    fn write_str(&mut self, string: &str) -> fmt::Result {
        if self.puts(string) {
            Ok(())
        } else {
            Err(fmt::Error {})
        }
    }
}

pub fn print_string_to_default_screen(args: fmt::Arguments) -> bool {
    if let Ok(mut graphic_manager) = get_kernel_manager_cluster().graphic_manager.try_lock() {
        use core::fmt::Write;
        if graphic_manager.write_fmt(args).is_ok() {
            return true;
        }
    }
    return false;
}

#[track_caller]
pub fn print_debug_message(level: usize, args: fmt::Arguments) -> bool {
    use core::panic::Location;
    let level_str = match level {
        3 => "[ERROR]",
        4 => "[WARN]",
        5 => "[NOTICE]",
        6 => "[INFO]",
        _ => "[???]",
    };
    let file = Location::caller().file(); //THINKING: filename only
    let line = Location::caller().line();
    return print_string_to_default_screen(format_args!(
        "{} {}:{} | {}",
        level_str, file, line, args
    ));
}

// macros
#[macro_export]
macro_rules! puts {
    ($fmt:expr) => {
        print!($fmt);
    };
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::kernel::graphic_manager::print_string_to_default_screen(format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! println {
    ($fmt:expr) => (print!(concat!($fmt,"\n")));
    ($fmt:expr, $($arg:tt)*) => (print!(concat!($fmt, "\n"),$($arg)*)); //\nをつける
}

#[macro_export]
macro_rules! kprintln {
    ($fmt:expr) => (print!(concat!($fmt,"\n")));
    ($fmt:expr, $($arg:tt)*) => (print!(concat!($fmt, "\n"),$($arg)*)); //\nをつける
}

#[macro_export]
macro_rules! pr_info {
    ($fmt:expr) => ($crate::kernel::graphic_manager::print_debug_message(6, format_args!(concat!($fmt,"\n"))));
    ($fmt:expr, $($arg:tt)*) => ($crate::kernel::graphic_manager::print_debug_message(6, format_args!(concat!($fmt, "\n"),$($arg)*))); //\nをつける
}

#[macro_export]
macro_rules! pr_warn {
    ($fmt:expr) => ($crate::kernel::graphic_manager::print_debug_message(4, format_args!(concat!($fmt,"\n"))));
    ($fmt:expr, $($arg:tt)*) => ($crate::kernel::graphic_manager::print_debug_message(4, format_args!(concat!($fmt, "\n"),$($arg)*))); //\nをつける
}

#[macro_export]
macro_rules! pr_err {
    ($fmt:expr) => ($crate::kernel::graphic_manager::print_debug_message(3, format_args!(concat!($fmt,"\n"))));
    ($fmt:expr, $($arg:tt)*) => ($crate::kernel::graphic_manager::print_debug_message(3, format_args!(concat!($fmt, "\n"),$($arg)*))); //\nをつける
}
