// Test support for cargo target clone seeding.
// Exports clone probe and copy overrides used by seed tests.
// Deps: std path/process helpers and thread-local override state.

use std::cell::RefCell;
use std::path::Path;
use std::process::Command;

thread_local! {
    static CLONE_PROBE_OVERRIDE: RefCell<Option<Result<(), String>>> = const { RefCell::new(None) };
    static REGULAR_COPY_OVERRIDE: RefCell<bool> = const { RefCell::new(false) };
}

pub(crate) struct CloneSeedGuard {
    previous_probe: Option<Result<(), String>>,
    previous_copy: bool,
}

impl CloneSeedGuard {
    pub(crate) fn regular_copy() -> Self {
        Self::set(Some(Ok(())), true)
    }

    pub(crate) fn unavailable(reason: &str) -> Self {
        Self::set(Some(Err(reason.to_string())), false)
    }

    fn set(probe: Option<Result<(), String>>, copy: bool) -> Self {
        let previous_probe = CLONE_PROBE_OVERRIDE.with(|cell| cell.borrow().clone());
        let previous_copy = REGULAR_COPY_OVERRIDE.with(|cell| *cell.borrow());
        CLONE_PROBE_OVERRIDE.with(|cell| *cell.borrow_mut() = probe);
        REGULAR_COPY_OVERRIDE.with(|cell| *cell.borrow_mut() = copy);
        Self { previous_probe, previous_copy }
    }
}

impl Drop for CloneSeedGuard {
    fn drop(&mut self) {
        CLONE_PROBE_OVERRIDE.with(|cell| *cell.borrow_mut() = self.previous_probe.take());
        REGULAR_COPY_OVERRIDE.with(|cell| *cell.borrow_mut() = self.previous_copy);
    }
}

pub(crate) fn clone_probe_override() -> Option<Result<(), String>> {
    CLONE_PROBE_OVERRIDE.with(|cell| cell.borrow().clone())
}

pub(crate) fn regular_copy_enabled() -> bool {
    REGULAR_COPY_OVERRIDE.with(|cell| *cell.borrow())
}

pub(crate) fn regular_copy_output(source: &Path, target: &Path) -> std::io::Result<std::process::Output> {
    regular_copy_path(source, target)?;
    Command::new("true").output()
}

fn regular_copy_path(source: &Path, target: &Path) -> std::io::Result<()> {
    if source.is_dir() {
        std::fs::create_dir_all(target)?;
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            regular_copy_path(&entry.path(), &target.join(entry.file_name()))?;
        }
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(source, target)?;
    Ok(())
}
