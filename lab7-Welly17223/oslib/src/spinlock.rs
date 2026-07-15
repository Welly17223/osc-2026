use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::sync::atomic;

use crate::interrupt;

pub struct SpinLock<T> {
    pub item: UnsafeCell<T>,
    lock: atomic::AtomicBool,
}

unsafe impl<T> Send for SpinLock<T> {}
unsafe impl<T> Sync for SpinLock<T> {}

pub struct SpinGuard<'a, T: 'a> {
    lock: &'a SpinLock<T>,
    _disable_interrupt: interrupt::SModeInterrupt,
    _marker: PhantomData<&'a mut T>,
}

impl<'a, T: 'a> SpinGuard<'a, T> {
    pub fn get_mut(&self) -> &'a mut T {
        unsafe { &mut *self.lock.item.get() }
    }

    pub fn get(&self) -> &'a T {
        unsafe { &*self.lock.item.get() }
    }
}

impl<'a, T> Drop for SpinGuard<'a, T> {
    fn drop(&mut self) {
        self.lock.lock.store(false, atomic::Ordering::SeqCst);
    }
}

impl<T> SpinLock<T> {
    pub fn new(item: T) -> Self {
        Self {
            item: UnsafeCell::new(item),
            lock: atomic::AtomicBool::new(false),
        }
    }

    pub fn obtain_lock(&self) {
        while let Err(val) = self.lock.compare_exchange(
            false,
            true,
            atomic::Ordering::SeqCst,
            atomic::Ordering::SeqCst,
        ) && val
        {}
    }

    pub fn lock(&self) -> SpinGuard<'_, T> {
        self.obtain_lock();
        SpinGuard {
            lock: self,
            _disable_interrupt: interrupt::SModeInterrupt::new(),
            _marker: PhantomData,
        }
    }

    /// # Safety
    /// You must known what you are doing
    pub unsafe fn unlock(&self) {
        self.lock.store(false, atomic::Ordering::Relaxed);
    }
}
