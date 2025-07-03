//!
//! The FIFO Queue
//!
//! This FIFO is lock-free.

use core::sync::atomic::{
    AtomicUsize,
    Ordering::{Acquire, Relaxed, Release},
    fence,
};

pub struct Fifo<T: Sized + Copy, const F_SIZE: usize> {
    buf: [T; F_SIZE],
    read_pointer: AtomicUsize,
    write_pointer: AtomicUsize,
    size: usize,
}

impl<T: Sized + Copy, const F_SIZE: usize> Fifo<T, F_SIZE> {
    pub const fn new(default_value: T) -> Self {
        Self {
            size: F_SIZE,
            buf: [default_value; F_SIZE],
            read_pointer: AtomicUsize::new(0),
            write_pointer: AtomicUsize::new(0),
        }
    }

    pub fn enqueue(&mut self, v: T) -> Result<(), ()> {
        loop {
            let write_pointer = self.write_pointer.load(Relaxed);
            let mut next_write_pointer = write_pointer + 1;
            if next_write_pointer >= self.size {
                next_write_pointer = 0;
            }
            if next_write_pointer == self.read_pointer.load(Relaxed) {
                return Err(());
            }

            /* This operation has an ABA problem. But usually buffer_full occurs first and it is rare. */
            if self
                .write_pointer
                .compare_exchange(write_pointer, next_write_pointer, Acquire, Relaxed)
                .is_ok()
            {
                self.buf[write_pointer] = v;
                fence(Release);
                return Ok(());
            }
        }
    }

    pub fn dequeue(&mut self) -> Option<T> {
        loop {
            let read_pointer = self.read_pointer.load(Relaxed);
            let mut next_read_pointer = read_pointer + 1;
            let write_pointer = self.write_pointer.load(Relaxed);
            if read_pointer == write_pointer {
                return None;
            }
            if next_read_pointer >= self.size {
                next_read_pointer = 0;
            }
            if self
                .read_pointer
                .compare_exchange(read_pointer, next_read_pointer, Acquire, Relaxed)
                .is_ok()
            {
                let result = self.buf[read_pointer];
                fence(Release);
                return Some(result);
            }
        }
    }
}
