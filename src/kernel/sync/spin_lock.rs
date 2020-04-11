/*
    Mutex(Spin Lock version)
*/

//use
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic;

pub struct Mutex<T: ?Sized> {
    lock_flag: atomic::AtomicBool,
    data: UnsafeCell<T>,
}

pub struct MutexGuard<'a, T: ?Sized + 'a> {
    // ドロップしても消さないように
    lock_flag: &'a atomic::AtomicBool,
    data: &'a mut T,
}

impl<T> Mutex<T> {
    pub const fn new(d: T) -> Mutex<T> {
        Mutex {
            lock_flag: atomic::AtomicBool::new(false),
            data: UnsafeCell::new(d),
        }
    }
}

impl<T: ?Sized> Mutex<T> {
    pub fn lock(&self) -> Result<MutexGuard<T>, ()> {
        self.lock_loop();
        Ok(MutexGuard {
            lock_flag: &self.lock_flag,
            data: unsafe { &mut *self.data.get() },
        }) //実質互換性のためにResultに包んでる
    }

    pub fn try_lock(&self) -> Result<MutexGuard<T>, ()> {
        if self.lock_flag.load(atomic::Ordering::Relaxed) {
            return Err(());
        }
        self.lock()
    }

    fn lock_loop(&self) {
        while !self
            .lock_flag
            .compare_and_swap(false, true, atomic::Ordering::Relaxed)
        {}
    }
}

impl<'m, T: ?Sized> Deref for MutexGuard<'m, T> {
    type Target = T;
    fn deref<'a>(&'a self) -> &'a T {
        &*self.data //参照外しのためのトレイト
    }
}

impl<'m, T: ?Sized> DerefMut for MutexGuard<'m, T> {
    fn deref_mut<'a>(&'a mut self) -> &'a mut T {
        &mut *self.data //参照外しのためのトレイト
    }
}

impl<'a, T: ?Sized> Drop for MutexGuard<'a, T> {
    //MutexGuardが削除されたとき自動的にロックを外す
    fn drop(&mut self) {
        self.lock_flag.store(false, atomic::Ordering::Release);
    }
}
