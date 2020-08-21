/*
 * VGA Text Mode Driver
 */

use arch::target_arch::device::crt;

use kernel::drivers::multiboot::FrameBufferInfo;
use kernel::graphic_manager::text_buffer_driver::TextBufferDriver;
use kernel::manager_cluster::get_kernel_manager_cluster;
use kernel::memory_manager::MemoryPermissionFlags;

pub struct VgaTextDriver {
    address: usize,
    width: usize,
    height: usize,
    cursor: TextCursor,
}

struct TextCursor {
    pub line: usize,
    pub character: usize,
    pub front_color: u32,
    pub back_color: u32,
}

impl VgaTextDriver {
    pub const fn new() -> Self {
        Self {
            address: 0,
            width: 0,
            height: 0,
            cursor: TextCursor {
                line: 0,
                character: 0,
                front_color: 0,
                back_color: 0,
            },
        }
    }

    pub fn init_by_multiboot_information(&mut self, frame_buffer_info: &FrameBufferInfo) -> bool {
        if frame_buffer_info.mode != 2 {
            return false;
        }
        self.address = frame_buffer_info.address as usize;
        self.width = frame_buffer_info.width as usize;
        self.height = frame_buffer_info.height as usize;
        self.cursor.front_color = 0xb; /* bright cyan */
        self.clear_screen();
        return true;
    }

    pub fn clear_screen(&mut self) {
        for i in 0..(self.width * self.height) {
            unsafe { *((self.address + i * 2) as *mut u16) = 0 };
        }
        crt::set_cursor_position(0);
    }

    pub fn set_frame_buffer_memory_permission(&mut self) -> bool {
        if self.address != 0 {
            match get_kernel_manager_cluster()
                .memory_manager
                .lock()
                .unwrap()
                .mmap_dev(
                    self.address,
                    self.width * self.height * 2 as usize,
                    MemoryPermissionFlags::data(),
                ) {
                Ok(address) => {
                    self.address = address;
                    true
                }
                Err(_) => false,
            }
        } else {
            false
        }
    }

    fn scroll_line(&mut self) {
        for i in 0..(self.width * (self.height - 1)) as usize {
            unsafe {
                *((self.address + i * 2) as *mut u16) =
                    *((self.address + (self.width as usize + i) * 2) as *mut u16);
            }
        }
        for i in (self.width * (self.height - 1))..(self.width * self.height) {
            unsafe {
                *((self.address + i as usize * 2) as *mut u16) = ' ' as u16;
            }
        }
        self.cursor.line -= 1;
        self.cursor.character = 0;
    }

    fn put_char(&self, c: u8) {
        /* For internal use(not moving pointer) */
        let t: u16 = ((self.cursor.back_color & 0x07) << 0x0C) as u16
            | ((self.cursor.front_color & 0x0F) << 0x08) as u16
            | c as u16;

        unsafe {
            *((self.address + (self.cursor.line * self.width + self.cursor.character) * 2)
                as *mut u16) = t;
        }
    }
}

impl TextBufferDriver for VgaTextDriver {
    fn puts(&mut self, string: &str) -> bool {
        for code in string.bytes() {
            match code as char {
                '\r' => self.cursor.character = 0,
                '\n' => {
                    unsafe {
                        *((self.address
                            + (self.cursor.line * self.width + self.cursor.character) * 2)
                            as *mut u16) = ' ' as u16;
                    } //暫定的な目印(カラーコードは0にすることで区別)
                    self.cursor.character = 0;
                    self.cursor.line += 1;
                }
                '\x08' => {
                    if self.cursor.character == 0 {
                        if self.cursor.line > 0 {
                            self.cursor.character = 0;
                            for x in 0..self.width {
                                if unsafe {
                                    *((self.address + (self.cursor.line * self.width - x) * 2)
                                        as *const u16)
                                        == ' ' as u16
                                } {
                                    self.cursor.character = self.width - x - 1;
                                    break;
                                }
                            }
                            self.cursor.line -= 1;
                        }
                    } else {
                        self.cursor.character -= 1;
                        self.put_char(' ' as u8);
                    }
                }
                _ => {
                    self.put_char(code);
                    self.cursor.character += 1;
                    if self.cursor.character >= self.width {
                        self.cursor.line += 1;
                        self.cursor.character = 0;
                    }
                }
            };
            if self.cursor.line >= self.height {
                self.scroll_line();
            }

            /* move the cursor by crt */
            crt::set_cursor_position(
                (self.cursor.line * self.width + self.cursor.character) as u16,
            );
        }
        return true;
    }
}
