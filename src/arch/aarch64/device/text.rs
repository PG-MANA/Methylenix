//!
//! Text Mode Driver
//!
//! This module is for compatibility of x86_64

use crate::kernel::drivers::multiboot::FrameBufferInfo;
use crate::kernel::graphic_manager::text_buffer_driver::TextBufferDriver;

pub struct TextDriver {}

impl TextDriver {
    pub const fn new() -> Self {
        Self {}
    }

    pub fn set_frame_buffer_memory_permission(&mut self) -> bool {
        unimplemented!()
    }

    pub fn init_by_multiboot_information(&mut self, _: &FrameBufferInfo) -> bool {
        unimplemented!()
    }

    pub fn clear_screen(&mut self) {
        unimplemented!()
    }
}

impl TextBufferDriver for TextDriver {
    fn puts(&mut self, _: &str) -> bool {
        return true;
    }
}
