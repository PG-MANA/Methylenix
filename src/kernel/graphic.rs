/*
    フォントの描画などをフレームバッファに行う
*/

// use(Arch非依存)
use core::fmt;
use kernel::drivers::multiboot::FrameBufferInfo;
use kernel::struct_manager::STATIC_BOOT_INFORMATION_MANAGER;

// use(Arch依存)
use arch::target_arch::device::crt;

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

    fn init_color_mode(&mut self) {
        //こっからテスト
        if self.frame_buffer_color_depth == 32 {
            self.is_textmode = false;
            // 文字見えてないだろうから#FF7F27で塗りつぶす
            for count in 0..(self.frame_buffer_width * self.frame_buffer_height) {
                unsafe {
                    *((self.frame_buffer_address + count * 4) as *mut u32) = 0xff7f27;
                }
            }
        }
    }

    pub fn init_vga_text_mode(&mut self) {
        self.is_textmode = true;
        self.cursor.front_color = 0xb; // bright cyan
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
        if self.is_textmode == false {
            let serial_port_manager_lock = unsafe {
                STATIC_BOOT_INFORMATION_MANAGER
                    .serial_port_manager
                    .try_lock()
            };
            if serial_port_manager_lock.is_ok() {
                serial_port_manager_lock.unwrap().sendstr(string);
                return true;
            }
            return false;
        }

        for code in string.bytes() {
            match code as char {
                '\r' => self.cursor.character = 0,
                '\n' => {
                    unsafe {
                        *((self.frame_buffer_address
                            + (self.cursor.line * self.frame_buffer_width + self.cursor.character)
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
                                            + (self.cursor.line * self.frame_buffer_width - x) * 2)
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
            // カーソル移動
            crt::set_cursor_position(
                (self.cursor.line * self.frame_buffer_width + self.cursor.character) as u16,
            );
        }
        true
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
    let graphic_manager_lock =
        unsafe { STATIC_BOOT_INFORMATION_MANAGER.graphic_manager.try_lock() };
    if graphic_manager_lock.is_ok() {
        use core::fmt::Write;
        if graphic_manager_lock.unwrap().write_fmt(args).is_ok() {
            return true;
        }
    }
    return false;
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
