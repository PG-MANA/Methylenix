//!
//! Text Buffer Driver(Trait)
//!
//! This trait is used to handle text based output driver
//! Don't have a string buffer, that should be done by stdio manager.

pub trait TextBufferDriver {
    fn puts(&mut self, s: &str) -> bool;
}

pub struct DummyTextBuffer {
    buffer: usize,
    pointer: usize,
    len: usize,
}

impl DummyTextBuffer {
    pub const fn new(buffer: usize, len: usize) -> Self {
        Self {
            buffer,
            pointer: 0,
            len,
        }
    }
}

impl TextBufferDriver for DummyTextBuffer {
    fn puts(&mut self, s: &str) -> bool {
        if self.buffer == 0 || self.pointer >= self.len {
            return false;
        }
        for c in s.as_bytes() {
            unsafe { *((self.buffer + self.pointer) as *mut u8) = *c };
            self.pointer += 1;
            if self.len >= self.pointer {
                return false;
            }
        }
        return true;
    }
}
