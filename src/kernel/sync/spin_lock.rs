//!
//! Mutex(Spin Lock version)
//!

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic;

pub struct Mutex<T: ?Sized> {
    lock_flag: SpinLockFlag,
    data: UnsafeCell<T>,
}

pub struct SpinLockFlag {
    flag: atomic::AtomicBool,
}

pub struct SpinLockFlagHolder {
    flag: usize,
}

pub struct MutexGuard<'a, T: ?Sized + 'a> {
    _lock_flag: SpinLockFlagHolder,
    data: &'a mut T,
}

impl<T> Mutex<T> {
    pub const fn new(d: T) -> Mutex<T> {
        Mutex {
            lock_flag: SpinLockFlag::new(),
            data: UnsafeCell::new(d),
        }
    }
}

impl SpinLockFlag {
    pub const fn new() -> Self {
        Self {
            flag: atomic::AtomicBool::new(false),
        }
    }

    pub fn try_lock(&self) -> Result<SpinLockFlagHolder, ()> {
        if self
            .flag
            .compare_exchange(
                false,
                true,
                atomic::Ordering::Relaxed,
                atomic::Ordering::Relaxed,
            )
            .is_ok()
        {
            Ok(SpinLockFlagHolder {
                flag: &self.flag as *const _ as usize,
            })
        } else {
            Err(())
        }
    }

    pub fn lock(&self) -> SpinLockFlagHolder {
        loop {
            let lock = self.try_lock();
            if lock.is_ok() {
                return lock.unwrap();
            }
        }
    }
}

impl Drop for SpinLockFlagHolder {
    fn drop(&mut self) {
        unsafe { &*(self.flag as *const atomic::AtomicBool) }
            .store(false, atomic::Ordering::Relaxed);
    }
}

impl<T: ?Sized> Mutex<T> {
    pub fn lock(&self) -> Result<MutexGuard<T>, ()> {
        let lock_holder = self.lock_flag.lock();
        Ok(MutexGuard {
            _lock_flag: lock_holder,
            data: unsafe { &mut *self.data.get() },
        })
    }

    pub fn try_lock(&self) -> Result<MutexGuard<T>, ()> {
        let result = self.lock_flag.try_lock();
        if result.is_err() {
            return Err(());
        }

        Ok(MutexGuard {
            _lock_flag: result.unwrap(),
            data: unsafe { &mut *self.data.get() },
        })
    }
}

impl<'m, T: ?Sized> Deref for MutexGuard<'m, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &*self.data
    }
}

impl<'m, T: ?Sized> DerefMut for MutexGuard<'m, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut *self.data
    }
}
