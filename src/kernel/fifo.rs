/*
FIFOシステム(暫定)
*/

use core::mem;

pub struct FIFO<T: Copy> {
    buf: [T; 128],
    r: usize,
    w: usize,
    size: usize,
    //暫定
    free: usize, //暫定
}

impl<T: Copy> FIFO<T> {
    pub fn new(_f_size: usize /*可変にできないので今の所無視*/) -> FIFO<T> {
        FIFO {
            size: 128,
            buf: unsafe { mem::uninitialized::<[T; 128]>() },
            r: 0,
            w: 0,
            free: 128,
        }
    }

    pub const fn new_static(_f_size: usize, default_value: &T) -> FIFO<T> {
        FIFO {
            size: 128,
            buf: [*default_value; 128],
            r: 0,
            w: 0,
            free: 128,
        }
    }

    pub fn queue(&mut self, v: T) -> bool {
        if self.free == 0 {
            return false;
        }
        self.buf[self.w] = v;
        self.w = if self.w + 1 == self.size {
            0
        } else {
            self.w + 1
        };
        self.free -= 1;
        true
    }

    pub fn dequeue(&mut self) -> Option<T> {
        if self.w == self.r {
            return None;
        }
        let result: T = unsafe { mem::transmute_copy(&self.buf[self.r]) }; //いい案ないかな
        self.r = if self.r + 1 == self.size {
            0
        } else {
            self.r + 1
        };
        self.free += 1;
        Some(result)
    }
}
