// Cargo target seed mechanics: clone-only copy probes, temp naming, cleanup.
// Exports seed outcomes plus helpers used by agent env wiring.
// Deps: std fs/process/path, libc clonefile on macOS, test-only override hooks.

use anyhow::Result;
#[cfg(test)]
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

static SEED_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BranchTargetSeedOutcome {
    Seeded { target: String, source: String, elapsed_ms: u128 },
    Skipped { target: String, reason: String },
}

#[cfg(test)]
thread_local! {
    static CLONE_PROBE_OVERRIDE: RefCell<Option<Result<(), String>>> = const { RefCell::new(None) };
    static REGULAR_COPY_OVERRIDE: RefCell<bool> = const { RefCell::new(false) };
}

#[cfg(test)]
pub(crate) struct CloneSeedGuard {
    previous_probe: Option<Result<(), String>>,
    previous_copy: bool,
}

#[cfg(test)]
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

#[cfg(test)]
impl Drop for CloneSeedGuard {
    fn drop(&mut self) {
        CLONE_PROBE_OVERRIDE.with(|cell| *cell.borrow_mut() = self.previous_probe.take());
        REGULAR_COPY_OVERRIDE.with(|cell| *cell.borrow_mut() = self.previous_copy);
    }
}

pub(crate) fn seed_branch_target_from_source(source: &Path, target: &Path) -> BranchTargetSeedOutcome {
    let target_display = target.to_string_lossy().into_owned();
    if target.exists() {
        return skipped(target_display, "destination exists");
    }
    if !source.is_dir() {
        return skipped(target_display, "base target directory is missing");
    }
    let Some(parent) = target.parent() else {
        return skipped(target_display, "destination has no parent directory");
    };
    if let Err(err) = std::fs::create_dir_all(parent) {
        return skipped(target_display, format!("failed to create parent directory: {err}"));
    }
    if let Err(reason) = clone_available(parent) {
        return skipped(target_display, reason);
    }
    clone_source_to_target(source, target, target_display)
}

pub(crate) fn remove_branch_target_dir(target: &Path) -> Result<bool> {
    if !target.exists() {
        return Ok(false);
    }
    std::fs::remove_dir_all(target)?;
    Ok(true)
}

fn clone_source_to_target(source: &Path, target: &Path, target_display: String) -> BranchTargetSeedOutcome {
    let temp_target = temp_seed_target(target);
    let start = Instant::now();
    let output = clone_dir(source, &temp_target);
    let output = match output {
        Ok(output) => output,
        Err(err) => return skipped(target_display, format!("failed to start clone copy: {err}")),
    };
    if !output.status.success() {
        let _ = std::fs::remove_dir_all(&temp_target);
        return skipped(target_display, format!("clone copy failed: {}", copy_stderr(&output.stderr)));
    }
    if let Err(err) = std::fs::rename(&temp_target, target) {
        let _ = std::fs::remove_dir_all(&temp_target);
        return skipped(target_display, format!("failed to move seeded target into place: {err}"));
    }
    BranchTargetSeedOutcome::Seeded {
        target: target_display,
        source: source.to_string_lossy().into_owned(),
        elapsed_ms: start.elapsed().as_millis(),
    }
}

fn temp_seed_target(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("target");
    let id = SEED_COUNTER.fetch_add(1, Ordering::Relaxed);
    target.with_file_name(format!(".{name}.seed-{}-{id}", std::process::id()))
}

fn clone_available(parent: &Path) -> Result<(), String> {
    #[cfg(test)]
    if let Some(result) = CLONE_PROBE_OVERRIDE.with(|cell| cell.borrow().clone()) {
        return result;
    }
    let id = SEED_COUNTER.fetch_add(1, Ordering::Relaxed);
    let source = parent.join(format!(".aid-clone-probe-src-{}-{id}", std::process::id()));
    let target = parent.join(format!(".aid-clone-probe-dst-{}-{id}", std::process::id()));
    std::fs::write(&source, b"x").map_err(|err| format!("clone probe setup failed: {err}"))?;
    let result = clone_probe_file(&source, &target);
    let _ = std::fs::remove_file(&source);
    let _ = std::fs::remove_file(&target);
    result
}

#[cfg(target_os = "macos")]
fn clone_probe_file(source: &Path, target: &Path) -> Result<(), String> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    unsafe extern "C" {
        fn clonefile(src: *const libc::c_char, dst: *const libc::c_char, flags: u32) -> libc::c_int;
    }

    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| "clone probe source path contains nul byte".to_string())?;
    let target = CString::new(target.as_os_str().as_bytes())
        .map_err(|_| "clone probe target path contains nul byte".to_string())?;
    let status = unsafe { clonefile(source.as_ptr(), target.as_ptr(), 0) };
    if status == 0 {
        Ok(())
    } else {
        Err(format!("clone unavailable: {}", std::io::Error::last_os_error()))
    }
}

#[cfg(target_os = "linux")]
fn clone_probe_file(source: &Path, target: &Path) -> Result<(), String> {
    let output = Command::new("cp")
        .arg("--reflink=always")
        .arg(source)
        .arg(target)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("clone probe failed to start: {err}"))?;
    output
        .status
        .success()
        .then_some(())
        .ok_or_else(|| format!("clone unavailable: {}", copy_stderr(&output.stderr)))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn clone_probe_file(_source: &Path, _target: &Path) -> Result<(), String> {
    Err("clone unavailable: unsupported platform".to_string())
}

fn clone_dir(source: &Path, target: &Path) -> std::io::Result<std::process::Output> {
    #[cfg(test)]
    if REGULAR_COPY_OVERRIDE.with(|cell| *cell.borrow()) {
        return regular_copy_dir_output(source, target);
    }
    clone_dir_command(source, target)
}

#[cfg(target_os = "linux")]
fn clone_dir_command(source: &Path, target: &Path) -> std::io::Result<std::process::Output> {
    Command::new("cp")
        .arg("-a")
        .arg("--reflink=always")
        .arg(source)
        .arg(target)
        .stdin(Stdio::null())
        .output()
}

#[cfg(not(target_os = "linux"))]
fn clone_dir_command(source: &Path, target: &Path) -> std::io::Result<std::process::Output> {
    Command::new("cp")
        .arg("-Rc")
        .arg(source)
        .arg(target)
        .stdin(Stdio::null())
        .output()
}

#[cfg(test)]
fn regular_copy_dir_output(source: &Path, target: &Path) -> std::io::Result<std::process::Output> {
    regular_copy_dir(source, target)?;
    Command::new("true").output()
}

#[cfg(test)]
fn regular_copy_dir(source: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let child_source = entry.path();
        let child_target = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            regular_copy_dir(&child_source, &child_target)?;
        } else {
            std::fs::copy(&child_source, &child_target)?;
        }
    }
    Ok(())
}

fn skipped(target: String, reason: impl Into<String>) -> BranchTargetSeedOutcome {
    BranchTargetSeedOutcome::Skipped { target, reason: reason.into() }
}

fn copy_stderr(stderr: &[u8]) -> String {
    let message = String::from_utf8_lossy(stderr).trim().to_string();
    if message.is_empty() {
        "cp clone exited unsuccessfully".to_string()
    } else {
        message
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_seed_target_is_unique_within_process() {
        let target = PathBuf::from("/tmp/cache/feat-shared");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let mut handles = Vec::new();
        for _ in 0..8 {
            let barrier = std::sync::Arc::clone(&barrier);
            let target = target.clone();
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                temp_seed_target(&target)
            }));
        }

        let mut paths = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        paths.sort();
        paths.dedup();
        assert_eq!(paths.len(), 8);
    }

    #[test]
    fn seed_skips_when_clone_probe_is_unavailable() {
        let _probe = CloneSeedGuard::unavailable("clone unavailable: forced");
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("_base");
        let target = temp.path().join("feat-cache");
        std::fs::create_dir_all(&source).unwrap();

        let outcome = seed_branch_target_from_source(&source, &target);

        assert_eq!(
            outcome,
            BranchTargetSeedOutcome::Skipped {
                target: target.to_string_lossy().into_owned(),
                reason: "clone unavailable: forced".to_string(),
            }
        );
        assert!(!target.exists());
    }
}
