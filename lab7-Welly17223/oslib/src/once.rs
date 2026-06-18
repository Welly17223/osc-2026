use core::{cell::OnceCell, sync::atomic};

pub struct Once<T> {
    is_init: atomic::AtomicBool,
    data: OnceCell<T>,
}

unsafe impl<T> Sync for Once<T> {}

impl<T> Once<T> {
    pub const fn new() -> Self {
        Once {
            is_init: atomic::AtomicBool::new(false),
            data: OnceCell::new(),
        }
    }

    pub fn get(&self) -> Option<&T> {
        self.data.get()
    }

    pub fn get_or_init<F>(&self, f: F) -> &T
    where
        F: FnOnce() -> T,
    {
        match self.is_init.compare_exchange(
            false,
            true,
            atomic::Ordering::SeqCst,
            atomic::Ordering::Relaxed,
        ) {
            Ok(_) => self.data.get_or_init(f),
            Err(_) => self.get().unwrap(),
        }
    }

    pub fn set(&self, value: T) -> Result<(), T> {
        self.data.set(value)
    }

    pub fn get_mut(&mut self) -> Option<&mut T> {
        self.data.get_mut()
    }
}

impl<T> Default for Once<T> {
    fn default() -> Self {
        Self::new()
    }
}
