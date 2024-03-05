//!
//! Mutex(Spin Lock version)
//!

use crate::arch::target_arch::interrupt::{InterruptManager, StoredIrqData};

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::ops::{Deref, DerefMut};
use core::panic::Location;
use core::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug)]
pub struct Mutex<T: ?Sized> {
    lock_flag: SpinLockFlag,
    data: UnsafeCell<T>,
}

#[derive(Debug)]
pub struct SpinLockFlag {
    flag: AtomicBool,
}

pub struct SpinLockFlagHolder {
    flag: *const AtomicBool,
}

pub struct IrqSaveSpinLockFlag {
    flag: AtomicBool,
}

pub struct IrqSaveSpinLockFlagHolder {
    flag: *const AtomicBool,
    irq: StoredIrqData,
}

pub struct ClassicIrqSaveSpinLockFlag {
    flag: AtomicBool,
    irq: UnsafeCell<MaybeUninit<StoredIrqData>>,
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
            flag: AtomicBool::new(false),
        }
    }

    pub fn try_lock(&self) -> Result<SpinLockFlagHolder, ()> {
        if self
            .flag
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            Ok(SpinLockFlagHolder {
                flag: &self.flag as *const _,
            })
        } else {
            Err(())
        }
    }

    pub fn try_lock_weak(&self) -> Result<SpinLockFlagHolder, ()> {
        if self
            .flag
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            Ok(SpinLockFlagHolder {
                flag: &self.flag as *const _,
            })
        } else {
            Err(())
        }
    }

    #[track_caller]
    pub fn lock(&self) -> SpinLockFlagHolder {
        loop {
            if let Ok(s) = self.try_lock_weak() {
                return s;
            }
            let mut count = 0usize;
            while self.flag.load(Ordering::Relaxed) {
                if count > 0x100000000 {
                    pr_warn!("May be dead lock: Caller: {:?}", Location::caller());
                    count = 0;
                }
                core::hint::spin_loop();
                count += 1;
            }
        }
    }

    pub fn is_locked(&self) -> bool {
        self.flag.load(Ordering::Relaxed)
    }
}

impl Drop for SpinLockFlagHolder {
    fn drop(&mut self) {
        unsafe { &*self.flag }.store(false, Ordering::Release);
    }
}

impl IrqSaveSpinLockFlag {
    pub const fn new() -> Self {
        Self {
            flag: AtomicBool::new(false),
        }
    }

    pub fn try_lock(&self) -> Result<IrqSaveSpinLockFlagHolder, ()> {
        let irq = InterruptManager::save_and_disable_local_irq();
        if self
            .flag
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            Ok(IrqSaveSpinLockFlagHolder {
                flag: &self.flag as *const _,
                irq,
            })
        } else {
            InterruptManager::restore_local_irq(irq);
            Err(())
        }
    }

    pub fn try_lock_weak(&self) -> Result<IrqSaveSpinLockFlagHolder, ()> {
        let irq = InterruptManager::save_and_disable_local_irq();
        if self
            .flag
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            Ok(IrqSaveSpinLockFlagHolder {
                flag: &self.flag as *const _,
                irq,
            })
        } else {
            InterruptManager::restore_local_irq(irq);
            Err(())
        }
    }

    #[track_caller]
    pub fn lock(&self) -> IrqSaveSpinLockFlagHolder {
        loop {
            if let Ok(s) = self.try_lock_weak() {
                return s;
            }
            let mut count = 0usize;
            while self.flag.load(Ordering::Relaxed) {
                if count > 0x100000000 {
                    pr_warn!("May be dead lock: Caller: {:?}", Location::caller());
                    count = 0;
                }
                core::hint::spin_loop();
                count += 1;
            }
        }
    }

    pub fn is_locked(&self) -> bool {
        self.flag.load(Ordering::Relaxed)
    }
}

impl Drop for IrqSaveSpinLockFlagHolder {
    fn drop(&mut self) {
        unsafe {
            (*self.flag).store(false, Ordering::Release);
            InterruptManager::restore_local_irq_by_reference(&self.irq);
        }
    }
}

impl ClassicIrqSaveSpinLockFlag {
    pub const fn new() -> Self {
        Self {
            flag: AtomicBool::new(false),
            irq: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    pub fn try_lock(&self) -> Result<(), ()> {
        let irq = InterruptManager::save_and_disable_local_irq();
        if self
            .flag
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            unsafe { self.irq.get().write(MaybeUninit::new(irq)) };
            Ok(())
        } else {
            InterruptManager::restore_local_irq(irq);
            Err(())
        }
    }

    pub fn try_lock_weak(&self) -> Result<(), ()> {
        let irq = InterruptManager::save_and_disable_local_irq();
        if self
            .flag
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            unsafe { self.irq.get().write(MaybeUninit::new(irq)) };
            Ok(())
        } else {
            InterruptManager::restore_local_irq(irq);
            Err(())
        }
    }

    pub fn lock(&self) {
        loop {
            if self.try_lock_weak().is_ok() {
                return;
            }
            while self.flag.load(Ordering::Relaxed) {
                core::hint::spin_loop();
            }
        }
    }

    pub fn unlock(&self) {
        assert!(self.is_locked());
        let irq = unsafe { self.irq.get().read().assume_init_read() };
        self.flag.store(false, Ordering::Release);
        InterruptManager::restore_local_irq(irq);
    }

    pub fn is_locked(&self) -> bool {
        self.flag.load(Ordering::Relaxed)
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

pub struct SequenceSpinLock {
    write_lock: SpinLockFlag,
    sequence: usize,
}

/* Non-smart implementation... */
pub struct SequenceSpinLockWriteHolder {
    lock: SpinLockFlagHolder,
}

pub struct SequenceSpinLockReadHolder {
    sequence: usize,
}

impl SequenceSpinLock {
    pub const fn new() -> Self {
        Self {
            write_lock: SpinLockFlag::new(),
            sequence: 0,
        }
    }

    pub fn write_lock(&mut self) -> SequenceSpinLockWriteHolder {
        let lock = self.write_lock.lock();
        self.sequence += 1;
        SequenceSpinLockWriteHolder { lock }
    }

    pub fn write_unlock(&mut self, holder: SequenceSpinLockWriteHolder) {
        core::sync::atomic::fence(Ordering::Release);
        drop(holder.lock);
    }

    pub fn read_start(&self) -> SequenceSpinLockReadHolder {
        loop {
            let sequence = unsafe { core::ptr::read_volatile(&self.sequence) };
            if (sequence & 1) == 0 {
                return SequenceSpinLockReadHolder { sequence };
            }
            core::hint::spin_loop()
        }
    }

    pub fn should_read_retry(&self, holder: SequenceSpinLockReadHolder) -> bool {
        let sequence = unsafe { core::ptr::read_volatile(&self.sequence) };
        holder.sequence != sequence
    }
}
