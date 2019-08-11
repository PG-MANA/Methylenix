/*
RWLock実装
std::sync::RWLockと互換を取る
ただし、poisonFlagを使用しないためpanic検出はできない
*/

//use
use core::sync::atomic;
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};

pub struct RwLock<T: ?Sized> {
    write_lock: atomic::AtomicBool,
    data: UnsafeCell<T>,
}
/*
#[derive(Clone)]
pub struct RwLockPtr<T: ?Sized> {
    write_lock: &'static mut atomic::AtomicBool,
    data: &'static mut T,
}
*/
pub struct RwLockReadGuard<'a, T: ?Sized + 'a> {
    data: &'a T,
}

pub struct RwLockWriteGuard<'a, T: ?Sized + 'a> {
    /*ドロップしても消さないように*/
    write_lock: &'a atomic::AtomicBool,
    data: &'a mut T,
}


impl<T> RwLock<T> {
    pub const fn new(d: T) -> RwLock<T> {
        RwLock {
            write_lock: atomic::AtomicBool::new(false),
            data: UnsafeCell::new(d),
        }
    }
}

impl<T: ?Sized> RwLock<T> {
    pub fn read(&self) -> Result<RwLockReadGuard<'_, T>, ()> {
        Ok(RwLockReadGuard {
            data: unsafe { &*self.data.get() },
        })
    }

    pub fn write(&self) -> Result<RwLockWriteGuard<'_, T>, ()> {
        self.lock_loop();
        Ok(RwLockWriteGuard {
            write_lock: &self.write_lock,
            data: unsafe { &mut *self.data.get() },
        }) //実質互換性のためにResultに包んでる
    }

    pub fn try_write(&self) -> Result<RwLockWriteGuard<'_, T>, ()> {
        if self.write_lock.load(atomic::Ordering::Relaxed) {
            return Err(());
        }
        self.write()
    }

    fn lock_loop(&self) {
        //self.lock_flag.load
        loop {
            if !self.write_lock.load(atomic::Ordering::Relaxed)
                && !self.write_lock.swap(true, atomic::Ordering::Acquire)
            /*flagがtrueでないならfalseであるはず*/
            {
                break;
            }
        }
    }
}

impl<T: ?Sized> Deref for RwLockReadGuard<'_, T> {
    type Target = T;

    fn deref<'a>(&'a self) -> &'a T {
        &*self.data
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
    //MutexGuardが削除されたとき自動的にロックを外す
    fn drop(&mut self) {
        self.write_lock.store(false, atomic::Ordering::Release);
    }
}
