//!
//! EFI Simple Text Output Protocol
//!

use super::super::EfiStatus;

#[repr(C)]
pub struct EfiSimpleTextOutputProtocol {
    reset: extern "efiapi" fn(&Self, bool) -> EfiStatus,
    pub output_string: extern "efiapi" fn(&Self, *const u16) -> EfiStatus,
    test_string: usize,
    query_mode: usize,
    set_mode: usize,
    set_attribute: usize,
    clear_screen: usize,
    set_cursor_position: usize,
    enable_cursor: usize,
    mode: usize,
}
