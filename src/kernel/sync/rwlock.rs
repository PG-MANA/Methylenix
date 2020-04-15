/*
 * RwLock(Spin Lock version)
 */

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub struct RwLock<T: ?Sized> {
    write_locked: AtomicBool,
    readers: AtomicUsize,
    data: UnsafeCell<T>,
    /*poison flag*/
}

pub struct RwLockReadGuard<'a, T: ?Sized + 'a> {
    readers: &'a AtomicUsize,
    data: &'a T,
}

pub struct RwLockWriteGuard<'a, T: ?Sized + 'a> {
    write_locked: &'a AtomicBool,
    data: &'a mut T,
}

impl<T> RwLock<T> {
    pub const fn new(d: T) -> RwLock<T> {
        RwLock {
            write_locked: AtomicBool::new(false),
            readers: AtomicUsize::new(0),
            data: UnsafeCell::new(d),
        }
    }
}

impl<T: ?Sized> RwLock<T> {
    pub fn read(&self) -> Result<RwLockReadGuard<'_, T>, ()> {
        loop {
            let lock = self.try_read();
            if lock.is_ok() {
                return lock;
            }
        }
    }

    pub fn try_read(&self) -> Result<RwLockReadGuard<'_, T>, ()> {
        if !self.write_locked.load(Ordering::Relaxed) {
            if self
                .readers
                .fetch_update(
                    |x| {
                        if x == usize::max_value() {
                            None
                        } else {
                            Some(x + 1)
                        }
                    },
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                )
                .is_ok()
            {
                return Ok(RwLockReadGuard {
                    readers: &self.readers,
                    data: unsafe { &*self.data.get() },
                });
            }
        }
        return Err(());
    }

    pub fn write(&self) -> Result<RwLockWriteGuard<'_, T>, ()> {
        loop {
            let lock = self.try_write();
            if lock.is_ok() {
                return lock;
            }
        }
    }

    pub fn try_write(&self) -> Result<RwLockWriteGuard<'_, T>, ()> {
        if self
            .write_locked
            .compare_and_swap(false, true, Ordering::Relaxed)
        {
            if self.readers.load(Ordering::Relaxed) != 0 {
                self.write_locked.store(false, Ordering::Relaxed);
                return Err(());
            }
            return Ok(RwLockWriteGuard {
                write_locked: &self.write_locked,
                data: unsafe { &mut *self.data.get() },
            });
        }
        return Err(());
    }
}

impl<T: ?Sized> Deref for RwLockReadGuard<'_, T> {
    type Target = T;
    fn deref<'a>(&'a self) -> &'a T {
        &*self.data
    }
}

impl<'a, T: ?Sized> Drop for RwLockReadGuard<'_, T> {
    fn drop(&mut self) {
        if self.readers.fetch_sub(1, Ordering::SeqCst) == 0 {
            panic!("RwLock was broken!");
        }
    }
}

impl<T: ?Sized> Deref for RwLockWriteGuard<'_, T> {
    type Target = T;

    fn deref<'a>(&'a self) -> &'a T {
        &*self.data
    }
}

impl<T: ?Sized> DerefMut for RwLockWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut *self.data
    }
}

impl<'a, T: ?Sized> Drop for RwLockWriteGuard<'_, T> {
    fn drop(&mut self) {
        self.write_locked.store(false, Ordering::Release);
    }
}
