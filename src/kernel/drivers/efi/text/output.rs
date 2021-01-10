//!
//! EFI Output Protocol Manager
//!

use super::super::{EfiStatus, EFI_SUCCESS};

#[repr(C)]
#[derive(Clone)]
pub struct EfiOutputProtocol {
    reset: extern "win64" fn(*const EfiOutputProtocol, bool) -> EfiStatus,
    output: extern "win64" fn(*const EfiOutputProtocol, *const u16) -> EfiStatus,
    test_string: usize,
    query_mode: usize,
    set_mode: usize,
    set_attribute: usize,
    clear_screen: usize,
    set_cursor_position: usize,
    enable_cursor: usize,
    mode: usize,
}

pub struct EfiTextOutputManager {
    protocol: *const EfiOutputProtocol,
}

impl EfiTextOutputManager {
    pub const fn new() -> Self {
        EfiTextOutputManager {
            protocol: 0 as *const EfiOutputProtocol,
        }
    }

    pub fn init(&mut self, output_protocol: *const EfiOutputProtocol) -> bool {
        self.protocol = output_protocol;
        return true;
    }

    pub fn reset(&self, extended_verification: bool) -> EfiStatus {
        unsafe { ((*(self.protocol)).reset)(self.protocol, extended_verification) }
    }

    pub fn output(&self, string: &str) -> EfiStatus {
        let mut buf = [0; 256];
        let mut pointer = 0;

        for x in string.encode_utf16() {
            if pointer >= buf.len() - 1 {
                let status = unsafe { ((*(self.protocol)).output)(self.protocol, buf.as_ptr()) };
                if status != EFI_SUCCESS {
                    return status;
                }
                pointer = 0;
            }
            buf[pointer] = x;
            pointer += 1;
        }
        buf[pointer] = 0;
        unsafe { ((*(self.protocol)).output)(self.protocol, buf.as_ptr()) }
    }
}
