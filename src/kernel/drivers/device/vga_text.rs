//!
//! VGA Text Mode Driver
//!
//! VGA text mode is one of the display modes, we can show text by putting ASCII code in the memory.
//! This mode will be enabled when boot from legacy BIOS. Under the UEFI BIOS, this mode will be unusable.
//!

use crate::kernel::memory_manager::{
    MemoryError,
    data_type::{Address, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress},
    io_remap,
};
use crate::kernel::sync::spin_lock::Mutex;
use crate::kernel::tty::{TextColor, Writer};

use core::ptr::{NonNull, copy, write_bytes, write_volatile};

/// the buffer of VGA text mode is `[u16; width * height]`
/// and each 16bit consists of front/back color code and ASCII code.
pub struct VgaTextDriver {
    inner: Mutex<Inner>,
}

struct Inner {
    address: usize,
    width: u16,
    height: u16,
    line: u16,
    character: u16,
    #[allow(dead_code)]
    crt: CrtAddress,
}

#[derive(Clone, Copy)]
pub enum CrtAddress {
    Io(u16),
    MmIo(NonNull<u8>),
}

impl Inner {
    pub const fn new(address: usize, width: u16, height: u16, crt: CrtAddress) -> Self {
        Self {
            address,
            width,
            height,
            line: 0,
            character: 0,
            crt,
        }
    }

    #[cfg(target_arch = "x86_64")]
    fn set_cursor_position(&self, pos: u16) {
        match self.crt {
            CrtAddress::Io(port) => {
                use crate::arch::target_arch::device::cpu;
                unsafe {
                    cpu::out_byte(port, 0x0e);
                    cpu::out_byte(port + 1, (pos >> 8) as u8);
                    cpu::out_byte(port, 0x0f);
                    cpu::out_byte(port + 1, (pos & 0xff) as u8);
                }
            }
            CrtAddress::MmIo(_) => unimplemented!(),
        }
    }

    #[cfg(not(target_arch = "x86_64"))]
    fn set_cursor_position(&self, _: u16) {
        unimplemented!()
    }

    fn clear_screen(&mut self) {
        if self.address == 0 {
            return;
        }
        for i in 0..(self.width * self.height) {
            unsafe { write_volatile((self.address + (i * 2) as usize) as *mut u16, 0) };
        }
        self.line = 0;
        self.character = 0;
        self.set_cursor_position(0);
    }

    fn remap_buffer(&mut self) -> Result<(), MemoryError> {
        if self.address != 0 {
            io_remap!(
                PAddress::new(self.address),
                MSize::new((self.width * self.height * 2) as usize),
                MemoryPermissionFlags::data(),
                MemoryOptionFlags::DO_NOT_FREE_PHYSICAL_ADDRESS
            )
            .map(|a| self.address = a.to_usize())
        } else {
            Ok(())
        }
    }

    /// Delete first line and move the other lines to each above.
    ///
    /// If `Self::address` is zero, this function does nothing.
    fn scroll_line(&mut self) {
        if self.address == 0 {
            return;
        }
        unsafe {
            copy(
                (self.address + (self.width * 2) as usize) as *const u16,
                self.address as *mut u16,
                (self.width * (self.height - 1)) as usize,
            ); /* Move each lines to above one */
            write_bytes(
                (self.address + (self.width * (self.height - 1) * 2) as usize) as *mut u16,
                0,
                self.width as usize,
            ); /* Clear the last line */
        };
        self.line -= 1;
        self.character = 0;
        self.set_cursor_position(self.line * self.width);
    }

    fn text_color_to_number(&self, color: TextColor) -> u16 {
        match color {
            TextColor::Black => 0x0,
            TextColor::Green => 0xa,  /* Light Green */
            TextColor::Cyan => 0xb,   /* Light Cyan */
            TextColor::Red => 0x4,    /* Light Red */
            TextColor::Orange => 0xc, /* Light Red */
            TextColor::Yellow => 0xe,
            TextColor::White => 0xf,
            TextColor::Custom(_) => 0xf, /* Give up... */
        }
    }

    /// Put a char to next of last char **without moving the cursor and updating**.
    fn put_char(&self, c: u8, front_color_number: u16, back_color_number: u16) {
        /* For internal use(not moving pointer) */
        let t =
            ((back_color_number & 0x07) << 0x0C) | ((front_color_number & 0x0F) << 0x08) | c as u16;

        unsafe {
            write_volatile(
                (self.address + ((self.line * self.width + self.character) * 2) as usize)
                    as *mut u16,
                t,
            );
        }
    }

    fn write(
        &mut self,
        buf: &[u8],
        foreground: TextColor,
        background: TextColor,
    ) -> core::fmt::Result {
        let foreground_color = self.text_color_to_number(foreground);
        let background_color = self.text_color_to_number(background);
        for code in buf {
            match *code as char {
                '\r' => self.character = 0,
                '\n' => {
                    /* The mark to return from the next line by backspace */
                    self.put_char(b' ', 0, 0);
                    self.character = 0;
                    self.line += 1;
                }
                '\x08' => {
                    if self.character == 0 {
                        if self.line > 0 {
                            self.character = 0;
                            for x in 0..self.width {
                                if unsafe {
                                    *((self.address + ((self.line * self.width - x) * 2) as usize)
                                        as *const u16)
                                        == ' ' as u16
                                } {
                                    self.character = self.width - x - 1;
                                    break;
                                }
                            }
                            self.line -= 1;
                        }
                    } else {
                        self.character -= 1;
                        self.put_char(b' ', 0, 0);
                    }
                }
                _ => {
                    self.put_char(*code, foreground_color, background_color);
                    self.character += 1;
                    if self.character >= self.width {
                        self.line += 1;
                        self.character = 0;
                    }
                }
            };
            if self.line >= self.height {
                self.scroll_line();
            }

            /* Move the cursor by crt */
            self.set_cursor_position(self.line * self.width + self.character);
        }
        Ok(())
    }
}

impl VgaTextDriver {
    pub fn new(address: usize, width: u16, height: u16, crt: CrtAddress) -> Self {
        Self {
            inner: Mutex::new(Inner::new(address, width, height, crt)),
        }
    }

    pub fn init(&self) {
        self.inner.lock().unwrap().clear_screen();
    }

    /// Map physical address of the buffer to virtual address with write permission.
    ///
    /// After enabling memory management system, accessing physical address causes page fault.
    /// To avoid it, we must call [`io_remap`] and reset video ram's address.
    pub fn remap_buffer(&self) -> Result<(), MemoryError> {
        self.inner.lock().unwrap().remap_buffer()
    }
}

impl Writer for VgaTextDriver {
    fn write(&self, buf: &[u8], foreground: TextColor, background: TextColor) -> core::fmt::Result {
        self.inner
            .lock()
            .unwrap()
            .write(buf, foreground, background)
    }
}
