// Shared test environment guards for process-global variables.
// Exports: Cargo target directory guard for parallel test safety.
// Deps: std::env and synchronization primitives.

use std::ffi::{OsStr, OsString};
use std::sync::{Mutex, MutexGuard, OnceLock};

pub(crate) struct CargoTargetDirGuard {
    lock: MutexGuard<'static, ()>,
    previous: Option<OsString>,
}

impl CargoTargetDirGuard {
    pub(crate) fn set(value: impl AsRef<OsStr>) -> Self {
        let lock = cargo_target_dir_lock();
        let previous = std::env::var_os("CARGO_TARGET_DIR");
        unsafe { std::env::set_var("CARGO_TARGET_DIR", value) };
        Self { lock, previous }
    }
}

impl Drop for CargoTargetDirGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => unsafe { std::env::set_var("CARGO_TARGET_DIR", value) },
            None => unsafe { std::env::remove_var("CARGO_TARGET_DIR") },
        }
    }
}

fn cargo_target_dir_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|err| err.into_inner())
}
