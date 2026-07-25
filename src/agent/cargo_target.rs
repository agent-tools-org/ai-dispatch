// Cargo target seed mechanics: clone-only copy probes, temp naming, cleanup.
// Exports seed outcomes plus helpers used by agent env wiring.
// Deps: std fs/process/path, libc clonefile on macOS, test-only override hooks.

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

static SEED_COUNTER: AtomicU64 = AtomicU64::new(0);
const ROOT_ARTIFACT_ENTRY_NAMES: [&str; 3] = ["debug", "release", ".rustc_info.json"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BranchTargetSeedOutcome {
    Seeded { target: String, source: String, elapsed_ms: u128 },
    Skipped { target: String, reason: String },
}

#[cfg(test)]
#[path = "cargo_target_test_support.rs"]
mod test_support;
#[cfg(test)]
pub(crate) use test_support::CloneSeedGuard;

pub(crate) fn seed_branch_target_from_source(source: &Path, target: &Path) -> BranchTargetSeedOutcome {
    let target_display = target.to_string_lossy().into_owned();
    if target.exists() {
        return skipped(target_display, "destination exists");
    }
    if !source.is_dir() {
        return skipped(target_display, "base target directory is missing");
    }
    if let Err(reason) = prepare_seed_target(target) {
        return skipped(target_display, reason);
    }
    clone_source_to_target(source, target, target_display)
}

pub(crate) fn seed_branch_target_from_root_entries(
    root: &Path,
    target: &Path,
) -> BranchTargetSeedOutcome {
    let target_display = target.to_string_lossy().into_owned();
    if target.exists() {
        return skipped(target_display, "destination exists");
    }
    let entries = root_artifact_entries(root);
    if entries.is_empty() {
        return skipped(
            target_display,
            "base target directory is missing and no root cargo artifacts are present",
        );
    }
    if let Err(reason) = prepare_seed_target(target) {
        return skipped(target_display, reason);
    }
    clone_entries_to_target(&entries, root, target, target_display)
}

pub(crate) fn remove_branch_target_dir(target: &Path) -> Result<bool> {
    if !target.exists() {
        return Ok(false);
    }
    std::fs::remove_dir_all(target)?;
    Ok(true)
}

fn root_artifact_entries(root: &Path) -> Vec<PathBuf> {
    ROOT_ARTIFACT_ENTRY_NAMES
        .iter()
        .map(|name| root.join(name))
        .filter(|path| path.exists())
        .collect()
}

fn prepare_seed_target(target: &Path) -> Result<(), String> {
    if target.exists() {
        return Err("destination exists".to_string());
    }
    let Some(parent) = target.parent() else {
        return Err("destination has no parent directory".to_string());
    };
    std::fs::create_dir_all(parent)
        .map_err(|err| format!("failed to create parent directory: {err}"))?;
    clone_available(parent)
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

fn clone_entries_to_target(
    entries: &[PathBuf],
    source_root: &Path,
    target: &Path,
    target_display: String,
) -> BranchTargetSeedOutcome {
    let temp_target = temp_seed_target(target);
    let start = Instant::now();
    if let Err(err) = std::fs::create_dir(&temp_target) {
        return skipped(target_display, format!("failed to create seed target: {err}"));
    }
    for source in entries {
        let Some(name) = source.file_name() else {
            let _ = std::fs::remove_dir_all(&temp_target);
            return skipped(target_display, "artifact entry has no file name");
        };
        let entry_target = temp_target.join(name);
        let output = clone_dir(source, &entry_target);
        let output = match output {
            Ok(output) => output,
            Err(err) => {
                let _ = std::fs::remove_dir_all(&temp_target);
                return skipped(target_display, format!("failed to start clone copy: {err}"));
            }
        };
        if !output.status.success() {
            let _ = std::fs::remove_dir_all(&temp_target);
            return skipped(target_display, format!("clone copy failed: {}", copy_stderr(&output.stderr)));
        }
    }
    if let Err(err) = std::fs::rename(&temp_target, target) {
        let _ = std::fs::remove_dir_all(&temp_target);
        return skipped(target_display, format!("failed to move seeded target into place: {err}"));
    }
    BranchTargetSeedOutcome::Seeded {
        target: target_display,
        source: source_root.to_string_lossy().into_owned(),
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
    if let Some(result) = test_support::clone_probe_override() {
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
    if test_support::regular_copy_enabled() {
        return test_support::regular_copy_output(source, target);
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
#[path = "cargo_target_tests.rs"]
mod tests;
