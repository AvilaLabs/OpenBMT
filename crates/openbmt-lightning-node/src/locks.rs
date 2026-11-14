//! Locks that a panic elsewhere does not turn into a second panic. The state
//! behind each lock is a plain value that stays whole after any single
//! change, so a poisoned lock is read through rather than refused.

use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// A `Mutex` taken through poison.
pub(crate) trait MutexExt<T> {
    fn lock_recover(&self) -> MutexGuard<'_, T>;
}

impl<T> MutexExt<T> for Mutex<T> {
    fn lock_recover(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// An `RwLock` taken through poison.
pub(crate) trait RwLockExt<T> {
    fn read_recover(&self) -> RwLockReadGuard<'_, T>;
    fn write_recover(&self) -> RwLockWriteGuard<'_, T>;
}

impl<T> RwLockExt<T> for RwLock<T> {
    fn read_recover(&self) -> RwLockReadGuard<'_, T> {
        self.read().unwrap_or_else(|p| p.into_inner())
    }

    fn write_recover(&self) -> RwLockWriteGuard<'_, T> {
        self.write().unwrap_or_else(|p| p.into_inner())
    }
}
