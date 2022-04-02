//!
//! VGA Text Mode Driver
//!
//! VGA text mode is one of the display modes, we can show text by putting ASCII code in the memory.
//! This mode will be enabled when boot from legacy BIOS. Under the UEFI BIOS, this mode will be unusable.

use crate::arch::target_arch::device::crt;
use crate::io_remap;

use crate::kernel::drivers::multiboot::FrameBufferInfo;
use crate::kernel::graphic_manager::text_buffer_driver::TextBufferDriver;
use crate::kernel::memory_manager::data_type::{
    Address, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress,
};

/// VgaTextDriver
///
/// This driver implements TextBufferDriver trait.
/// the buffer of VGA text mode is \[u16; width * height\]
/// and each 16bit consists of front/back color code and ASCII code.
pub struct TextDriver {
    address: usize,
    width: usize,
    height: usize,
    cursor: TextCursor,
}

/// TextCursor
///
/// This struct contains which line and character the driver should put data.
/// Currently, front/back color is invariable but in the future it may be able to change it by control code.
struct TextCursor {
    pub line: usize,
    pub character: usize,
    pub front_color: u32,
    pub back_color: u32,
}

impl TextDriver {
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

    /// Init this driver with the multiboot information.
    ///
    /// Multiboot's frame buffer information has current screen's height, width and video ram address.
    /// if frame_buffer_info.mode != 2 (it means the screen is not text mode), this function will return false.
    /// after set the member variable, this clear screen.
    /// Default text front color is bright cyan(0xb) and back color is black(0x0).
    pub fn init_by_multiboot_information(&mut self, frame_buffer_info: &FrameBufferInfo) -> bool {
        if frame_buffer_info.mode != 2 {
            return false;
        }
        self.address = frame_buffer_info.address as usize;
        self.width = frame_buffer_info.width as usize;
        self.height = frame_buffer_info.height as usize;
        self.cursor.front_color = 0xb; /* Bright Cyan */
        self.clear_screen();
        return true;
    }

    /// Delete all text on the screen.
    ///
    /// This function deletes all characters from display and set cursor position to zero(top-left).
    /// If the screen is not text mode, this will do nothing.
    pub fn clear_screen(&mut self) {
        if self.address == 0 {
            return;
        }
        for i in 0..(self.width * self.height) {
            unsafe { *((self.address + i * 2) as *mut u16) = 0 };
        }
        crt::set_cursor_position(0);
    }

    /// Map physical address of video ram to virtual address with write permission.
    ///
    /// After enabling memory management system, accessing physical address causes page fault.
    /// To avoid it, we must call mmap_dev and reset video ram's address.
    /// This function returns true when mmap_dev is succeeded
    /// otherwise return false including the situation the screen is not text mode.
    pub fn set_frame_buffer_memory_permission(&mut self) -> bool {
        if self.address != 0 {
            match io_remap!(
                PAddress::new(self.address),
                MSize::new(self.width * self.height * 2 as usize),
                MemoryPermissionFlags::data(),
                MemoryOptionFlags::DO_NOT_FREE_PHYSICAL_ADDRESS
            ) {
                Ok(address) => {
                    self.address = address.to_usize();
                    true
                }
                Err(_) => false,
            }
        } else {
            false
        }
    }

    /// Delete first line and move the other lines to each above.
    ///
    /// If self.address == 0(not set up), this function does nothing.
    fn scroll_line(&mut self) {
        use core::ptr::{copy, write_bytes};
        if self.address == 0 {
            return;
        }
        unsafe {
            copy(
                (self.address + self.width * 2) as *const u16,
                self.address as *mut u16,
                self.width * (self.height - 1),
            ); /* Move each lines to above one */
            write_bytes(
                (self.address + self.width * (self.height - 1) * 2) as *mut u16,
                0,
                self.width,
            ); /* Clear the last line */
        };
        self.cursor.line -= 1;
        self.cursor.character = 0;
        /* Move the cursor by crt */
        crt::set_cursor_position((self.cursor.line * self.width) as u16);
    }

    /// Put a char to next of last char **without moving the cursor and updating**.
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

impl TextBufferDriver for TextDriver {
    fn puts(&mut self, string: &str) -> bool {
        for code in string.bytes() {
            match code as char {
                '\r' => self.cursor.character = 0,
                '\n' => {
                    unsafe {
                        *((self.address
                            + (self.cursor.line * self.width + self.cursor.character) * 2)
                            as *mut u16) = ' ' as u16;
                    } /* The mark to return from the next line by backspace */
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

            /* Move the cursor by crt */
            crt::set_cursor_position(
                (self.cursor.line * self.width + self.cursor.character) as u16,
            );
        }
        return true;
    }
}
