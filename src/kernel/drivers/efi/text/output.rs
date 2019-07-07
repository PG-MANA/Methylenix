// Grub2(Efi)では使えない...ハンドラをオープンする必要あり..?(ブート時に警告出てるし)

//use
use super::super::table::EfiStatus;

#[repr(C)]
#[derive(Clone)]
pub struct EfiOutputProtocol {
    reset: fn(*const EfiOutputProtocol, bool) -> EfiStatus,
    output: fn(*const EfiOutputProtocol, *const u16) -> EfiStatus,
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
        unsafe { ((*(self.protocol)).reset)(self.protocol, extended_verification) }
    }

    pub fn output(&self, string: &str) -> EfiStatus {
        let mut buf = [0 as u16; 256];
        let mut counter = 0;
        for x in string.encode_utf16() {
            if counter >= buf.len() - 1 {
                break;
            }
            buf[counter] = x;
            counter += 1;
        }
        unsafe { ((*(self.protocol)).output)(self.protocol, buf.as_ptr()) }
    }
}
