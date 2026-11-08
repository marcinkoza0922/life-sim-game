//! Runtime borrow tracking shared by component columns and resources.

use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicIsize, Ordering};

#[derive(Default, Debug)]
pub(crate) struct BorrowFlag(AtomicIsize);

impl BorrowFlag {
    pub fn try_shared(&self) -> bool {
        let mut cur = self.0.load(Ordering::Relaxed);
        loop {
            if cur < 0 {
                return false;
            }
            match self.0.compare_exchange_weak(cur, cur + 1, Ordering::Acquire, Ordering::Relaxed) {
                Ok(_) => return true,
                Err(v) => cur = v,
            }
        }
    }
    pub fn try_unique(&self) -> bool {
        self.0.compare_exchange(0, -1, Ordering::Acquire, Ordering::Relaxed).is_ok()
    }
    pub fn release_shared(&self) {
        let prev = self.0.fetch_sub(1, Ordering::Release);
        debug_assert!(prev > 0);
    }
    pub fn release_unique(&self) {
        let prev = self.0.swap(0, Ordering::Release);
        debug_assert_eq!(prev, -1);
    }
    pub fn acquire(&self, mutable: bool, what: &str) {
        let ok = if mutable { self.try_unique() } else { self.try_shared() };
        if !ok {
            panic!(
                "hearth_ecs: borrow conflict on `{what}` ({} borrow requested while {} borrow is held)",
                if mutable { "mutable" } else { "shared" },
                if self.0.load(Ordering::Relaxed) < 0 { "a mutable" } else { "a shared" }
            );
        }
    }
    pub fn release(&self, mutable: bool) {
        if mutable { self.release_unique() } else { self.release_shared() }
    }
}

/// Shared borrow of a component or resource; released on drop.
pub struct Ref<'a, T: ?Sized> {
    pub(crate) value: &'a T,
    pub(crate) flag: &'a BorrowFlag,
}

impl<T: ?Sized> Deref for Ref<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.value
    }
}
impl<T: ?Sized> Drop for Ref<'_, T> {
    fn drop(&mut self) {
        self.flag.release_shared();
    }
}
impl<T: ?Sized + std::fmt::Debug> std::fmt::Debug for Ref<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.value.fmt(f)
    }
}

/// Unique borrow of a component or resource; released on drop.
pub struct RefMut<'a, T: ?Sized> {
    pub(crate) value: &'a mut T,
    pub(crate) flag: &'a BorrowFlag,
}

impl<T: ?Sized> Deref for RefMut<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.value
    }
}
impl<T: ?Sized> DerefMut for RefMut<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.value
    }
}
impl<T: ?Sized> Drop for RefMut<'_, T> {
    fn drop(&mut self) {
        self.flag.release_unique();
    }
}
