// Grub2(Efi)では使えない...ハンドラをオープンする必要あり..?(ブート時に警告出てるし)

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
    mode: usize,
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
            (*((*self.protocol).reset as *const fn(usize, u8) -> EfiStatus))(
                self.protocol as usize,
                extended_verification as u8,
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
