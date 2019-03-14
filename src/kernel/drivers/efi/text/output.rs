// Grub2-Efiでは使えない

//use
use super::super::table::EfiStatus;

#[repr(C)]
#[derive(Clone)]
pub struct EfiOutputProtocol {
    reset: usize,
    output: usize,
    test_string: usize,
    query_mode: usize,
    set_mode: usize,
    set_attribute: usize,
    clear_screen: usize,
    set_cursor_position: usize,
    enable_cursor: usize,
}

pub struct EfiTextOutputManager {
    protocol: *const EfiOutputProtocol,
}

impl EfiTextOutputManager {
    pub fn new(output_protocol: *const EfiOutputProtocol) -> EfiTextOutputManager {
        EfiTextOutputManager {
            protocol: output_protocol,
        }
    }

    pub const fn new_static() -> EfiTextOutputManager {
        EfiTextOutputManager {
            protocol: 0 as *const EfiOutputProtocol,
        }
    }

    pub fn reset(&self, extended_verification: bool) -> EfiStatus {
        unsafe {
            (*((*self.protocol).reset as *const fn(*const EfiOutputProtocol, bool) -> EfiStatus))(
                self.protocol,
                extended_verification,
            )
        }
    }

    pub fn output(&self, string: &str) -> EfiStatus {
        unsafe {
            (*((*self.protocol).reset as *const fn(*const EfiOutputProtocol, &str) -> EfiStatus))(
                self.protocol,
                string,
            )
        }
    }
}
