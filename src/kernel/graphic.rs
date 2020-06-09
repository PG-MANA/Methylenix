/*
 * Graphic Manager
 * いずれはstdio.rsみたいなのを作ってそれのサブモジュールにしたい
 */

use arch::target_arch::device::crt;

use kernel::drivers::multiboot::FrameBufferInfo;
use kernel::manager_cluster::get_kernel_manager_cluster;
use kernel::memory_manager::MemoryPermissionFlags;

use core::fmt;

pub struct GraphicManager {
    frame_buffer_address: usize,
    frame_buffer_width: usize,
    frame_buffer_height: usize,
    frame_buffer_color_depth: u8,
    is_textmode: bool,
    cursor: TextCursor,
}

struct TextCursor {
    pub line: usize,
    pub character: usize,
    pub front_color: u32,
    pub back_color: u32,
}

impl GraphicManager {
    pub fn new(frame_buffer_info: &FrameBufferInfo) -> GraphicManager {
        let mut graphic_manager = GraphicManager::new_static();
        graphic_manager.init_manager(frame_buffer_info);
        graphic_manager
    }

    pub const fn new_static() -> GraphicManager {
        GraphicManager {
            frame_buffer_address: 0,
            frame_buffer_width: 0,
            frame_buffer_height: 0,
            frame_buffer_color_depth: 0,
            is_textmode: false,
            cursor: TextCursor {
                line: 0,
                character: 0,
                front_color: 0xffffff,
                back_color: 0,
            },
        }
    }

    pub fn set_frame_buffer_memory_permission(&mut self) -> bool {
        let pixel_size = if self.is_textmode {
            2
        } else {
            self.frame_buffer_color_depth / 8
        };
        match get_kernel_manager_cluster()
            .memory_manager
            .lock()
            .unwrap()
            .mmap_dev(
                self.frame_buffer_address,
                self.frame_buffer_height * self.frame_buffer_width * pixel_size as usize,
                MemoryPermissionFlags::data(),
            ) {
            Ok(address) => {
                self.frame_buffer_address = address;
                true
            }
            Err(_) => false,
        }
    }

    fn init_manager(&mut self, frame_buffer_info: &FrameBufferInfo) {
        self.frame_buffer_address = frame_buffer_info.address as usize;
        self.frame_buffer_width = frame_buffer_info.width as usize;
        self.frame_buffer_height = frame_buffer_info.height as usize;
        self.frame_buffer_color_depth = frame_buffer_info.depth;
        self.init_color_mode();
        match frame_buffer_info.mode {
            1 => self.init_color_mode(),
            2 => self.init_vga_text_mode(),
            _ => (/*何しよう...*/),
        }
    }

    pub fn init_color_mode(&mut self) {
        //こっからテスト
        if self.frame_buffer_color_depth == 32 {
            self.is_textmode = false;
            // 文字見えてないだろうから#FF7F27で塗りつぶす
            for count in 0..(self.frame_buffer_width * self.frame_buffer_height) {
                unsafe {
                    *((self.frame_buffer_address + count * 4) as *mut u32) = 0xff7f27;
                }
            }
        } else if self.frame_buffer_color_depth == 24 {
            self.is_textmode = false;
            // 文字見えてないだろうから#FF7F27で塗りつぶす
            for count in 0..(self.frame_buffer_width * self.frame_buffer_height) {
                unsafe {
                    let pixcel = (self.frame_buffer_address + count * 3) as *mut u32;
                    *pixcel &= 0x000000ff;
                    *pixcel |= 0xff7f27;
                }
            }
        }
    }

    pub fn init_vga_text_mode(&mut self) {
        self.is_textmode = true;
        self.cursor.front_color = 0xb; // bright cyan
        crt::set_cursor_position(0);
    }

    pub fn putchar(&self, c: char) {
        //カーソル操作はしない
        let t: u16 = ((self.cursor.back_color & 0x07) << 0x0C) as u16
            | ((self.cursor.front_color & 0x0F) << 0x08) as u16
            | (c as u8 & 0x00FF) as u16;
        unsafe {
            *((self.frame_buffer_address
                + (self.cursor.line * self.frame_buffer_width + self.cursor.character) * 2)
                as *mut u16) = t;
        }
    }

    pub fn puts(&mut self, string: &str) -> bool {
        get_kernel_manager_cluster()
            .serial_port_manager
            .sendstr(string);
        if self.is_textmode {
            for code in string.bytes() {
                match code as char {
                    '\r' => self.cursor.character = 0,
                    '\n' => {
                        unsafe {
                            *((self.frame_buffer_address
                                + (self.cursor.line * self.frame_buffer_width
                                    + self.cursor.character)
                                    * 2) as *mut u16) = ' ' as u16;
                        } //暫定的な目印(カラーコードは0にすることで区別)
                        self.cursor.character = 0;
                        self.cursor.line += 1;
                    }
                    '\x08' => {
                        if self.cursor.character == 0 {
                            if self.cursor.line > 0 {
                                self.cursor.character = 0;
                                for x in 0..self.frame_buffer_width {
                                    if unsafe {
                                        *((self.frame_buffer_address
                                            + (self.cursor.line * self.frame_buffer_width - x) * 2)
                                            as *const u16)
                                            == ' ' as u16
                                    } {
                                        self.cursor.character = self.frame_buffer_width - x - 1;
                                        unsafe {
                                            *((self.frame_buffer_address
                                                + (self.cursor.line * self.frame_buffer_width - x)
                                                    * 2)
                                                as *mut u16) = 0; //目印の削除
                                        }
                                        break;
                                    }
                                }
                                self.cursor.line -= 1;
                            }
                        } else {
                            self.cursor.character -= 1;
                        }
                        self.putchar(' ');
                    }
                    c => {
                        self.putchar(c);
                        self.cursor.character += 1;
                        if self.cursor.character >= self.frame_buffer_width {
                            self.cursor.line += 1;
                            self.cursor.character = 0;
                            //溢れ対策してない
                        }
                    }
                };
                if self.cursor.line >= self.frame_buffer_height {
                    for i in 0..(self.frame_buffer_width * (self.frame_buffer_height - 1)) as usize
                    {
                        unsafe {
                            *((self.frame_buffer_address + i * 2) as *mut u16) = *((self
                                .frame_buffer_address
                                + (self.frame_buffer_width as usize + i) * 2)
                                as *mut u16);
                        }
                    }
                    for i in (self.frame_buffer_width * (self.frame_buffer_height - 1))
                        ..(self.frame_buffer_width * self.frame_buffer_height)
                    {
                        unsafe {
                            *((self.frame_buffer_address + i as usize * 2) as *mut u16) =
                                ' ' as u16;
                        }
                    }
                    self.cursor.line -= 1;
                    self.cursor.character = 0;
                }
            }
            // カーソル移動
            crt::set_cursor_position(
                (self.cursor.line * self.frame_buffer_width + self.cursor.character) as u16,
            );
        }
        true
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
        if self.is_textmode {
            return false;
        }
        if (depth != 32 && depth != 24)
            || (self.frame_buffer_color_depth != 32 && self.frame_buffer_color_depth != 24)
        {
            return false;
        }
        let screen_depth_byte = self.frame_buffer_color_depth as usize / 8;
        let bitmap_depth_byte = depth as usize / 8;
        let bitmap_aligned_bitmap_width_pointer = ((size_x * bitmap_depth_byte - 1) & !3) + 4;
        if self.frame_buffer_color_depth == 32 {
            for height_pointer in (0..size_y).rev() {
                for width_pointer in 0..size_x {
                    unsafe {
                        *((self.frame_buffer_address
                            + ((height_pointer + offset_y) * self.frame_buffer_width
                                + offset_x
                                + width_pointer)
                                * screen_depth_byte) as *mut u32) = *((buffer
                            + (size_y - height_pointer - 1) * bitmap_aligned_bitmap_width_pointer
                            + width_pointer * bitmap_depth_byte)
                            as *const u32);
                    }
                }
            }
        } else {
            for height_pointer in (0..size_y).rev() {
                for width_pointer in 0..size_x {
                    unsafe {
                        let dot = (self.frame_buffer_address
                            + ((height_pointer + offset_y) * self.frame_buffer_width
                                + offset_x
                                + width_pointer)
                                * screen_depth_byte) as *mut u32;
                        *dot &= 0x000000ff;
                        *dot |= *((buffer
                            + (size_y - height_pointer) * bitmap_aligned_bitmap_width_pointer
                            + width_pointer * bitmap_depth_byte)
                            as *const u32)
                            & 0xffffff;
                    }
                }
            }
        }
        return true;
    }
}

impl fmt::Write for GraphicManager {
    fn write_str(&mut self, string: &str) -> fmt::Result {
        if self.puts(string) {
            return Ok(());
        } else {
            return Err(fmt::Error {});
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
        $crate::kernel::graphic::print_string_to_default_screen(format_args!($($arg)*));
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
    ($fmt:expr) => ($crate::kernel::graphic::print_debug_message(6, format_args!(concat!($fmt,"\n"))));
    ($fmt:expr, $($arg:tt)*) => ($crate::kernel::graphic::print_debug_message(6, format_args!(concat!($fmt, "\n"),$($arg)*))); //\nをつける
}

#[macro_export]
macro_rules! pr_warn {
    ($fmt:expr) => ($crate::kernel::graphic::print_debug_message(4, format_args!(concat!($fmt,"\n"))));
    ($fmt:expr, $($arg:tt)*) => ($crate::kernel::graphic::print_debug_message(4, format_args!(concat!($fmt, "\n"),$($arg)*))); //\nをつける
}

#[macro_export]
macro_rules! pr_err {
    ($fmt:expr) => ($crate::kernel::graphic::print_debug_message(3, format_args!(concat!($fmt,"\n"))));
    ($fmt:expr, $($arg:tt)*) => ($crate::kernel::graphic::print_debug_message(3, format_args!(concat!($fmt, "\n"),$($arg)*))); //\nをつける
}
