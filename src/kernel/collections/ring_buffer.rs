//!
//! Ring Buffer
//!

use crate::kernel::memory_manager::data_type::{Address, MSize, VAddress};

pub struct Ringbuffer {
    buffer: VAddress,
    size: MSize,
    write_ptr: usize,
    read_ptr: usize,
}

impl Ringbuffer {
    pub const fn new() -> Self {
        Self {
            buffer: VAddress::new(0),
            size: MSize::new(0),
            write_ptr: 0,
            read_ptr: 0,
        }
    }

    pub const fn get_buffer_address(&self) -> VAddress {
        self.buffer
    }

    pub const fn get_buffer_size(&self) -> MSize {
        self.size
    }

    pub fn set_new_buffer(&mut self, buffer: VAddress, size: MSize) {
        assert_ne!(size, MSize::new(0));
        assert!(size.to_usize().is_power_of_two());
        self.buffer = buffer;
        self.size = size;
        self.write_ptr = 0;
        self.read_ptr = size.to_usize() - 1;
    }

    pub fn unset_buffer(&mut self) {
        *self = Self::new()
    }

    fn add_pointer(&self, p: usize, v: usize) -> usize {
        (p + v) & (self.size.to_usize() - 1)
    }

    pub fn get_writable_size(&self) -> MSize {
        if self.read_ptr == self.write_ptr {
            MSize::new(0)
        } else if self.write_ptr < self.read_ptr {
            MSize::new(self.read_ptr - self.write_ptr + 1)
        } else {
            self.size - MSize::new(self.write_ptr - self.read_ptr - 1)
        }
    }

    pub fn get_readable_size(&self) -> MSize {
        self.size - self.get_writable_size()
    }

    pub fn write(&mut self, buffer: VAddress, size: MSize) -> MSize {
        let size = size.min(self.get_writable_size());
        if size.is_zero() {
            return size;
        }
        let start = self.write_ptr;
        self.write_ptr = self.add_pointer(self.write_ptr, size.to_usize());

        unsafe {
            core::ptr::copy_nonoverlapping(
                buffer.to_usize() as *const u8,
                (self.buffer.to_usize() + start) as *mut u8,
                (self.buffer.to_usize() - start).min(size.to_usize()),
            )
        };
        if start >= self.write_ptr {
            unsafe {
                core::ptr::copy_nonoverlapping(
                    (buffer.to_usize() + (self.buffer.to_usize() - start)) as *const u8,
                    self.buffer.to_usize() as *mut u8,
                    self.write_ptr,
                )
            };
        }
        size
    }

    pub fn read(&mut self, buffer: VAddress, size: MSize) -> MSize {
        let size = size.min(self.get_readable_size());
        if size.is_zero() {
            return size;
        }
        let start = self.add_pointer(self.read_ptr, 1);
        self.read_ptr = self.add_pointer(self.read_ptr, size.to_usize());
        unsafe {
            core::ptr::copy_nonoverlapping(
                (self.buffer.to_usize() + start) as *const u8,
                buffer.to_usize() as *mut u8,
                (self.buffer.to_usize() - start).min(size.to_usize()),
            )
        };
        if start >= self.read_ptr {
            unsafe {
                core::ptr::copy_nonoverlapping(
                    self.buffer.to_usize() as *const u8,
                    (buffer.to_usize() + self.buffer.to_usize() - start) as *mut u8,
                    self.read_ptr + 1,
                )
            };
        }
        size
    }
}
