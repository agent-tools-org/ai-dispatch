// Shared test environment guards for process-global variables and fixture cleanup.
// Exports: Cargo target dir guard; legacy /tmp worktree Drop guard.
// Deps: std::env/fs/path/process, synchronization primitives.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard, OnceLock};

pub(crate) struct CargoTargetDirGuard {
    _lock: MutexGuard<'static, ()>,
    previous: Option<OsString>,
}

impl CargoTargetDirGuard {
    pub(crate) fn set(value: impl AsRef<OsStr>) -> Self {
        let lock = cargo_target_dir_lock();
        let previous = std::env::var_os("CARGO_TARGET_DIR");
        unsafe { std::env::set_var("CARGO_TARGET_DIR", value) };
        Self { _lock: lock, previous }
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

pub(crate) struct FallbackTargetDirGuard {
    _lock: MutexGuard<'static, ()>,
    previous: Option<OsString>,
}

impl FallbackTargetDirGuard {
    pub(crate) fn set(value: impl AsRef<OsStr>) -> Self {
        let lock = fallback_target_dir_lock();
        let previous = std::env::var_os("AID_TEST_FALLBACK_TARGET_ROOT");
        unsafe { std::env::set_var("AID_TEST_FALLBACK_TARGET_ROOT", value) };
        Self { _lock: lock, previous }
    }
}

impl Drop for FallbackTargetDirGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => unsafe { std::env::set_var("AID_TEST_FALLBACK_TARGET_ROOT", value) },
            None => unsafe { std::env::remove_var("AID_TEST_FALLBACK_TARGET_ROOT") },
        }
    }
}

fn fallback_target_dir_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|err| err.into_inner())
}

/// RAII cleanup for a real git worktree (or plain dir) created under a fixed path.
/// Drop always runs `git worktree remove --force` (when a repo is known) then
/// `remove_dir_all`, so panicking tests cannot leave `/tmp/aid-wt-*` fixtures behind.
pub(crate) struct TmpWorktreeGuard {
    repo: Option<PathBuf>,
    path: PathBuf,
}

impl TmpWorktreeGuard {
    pub(crate) fn with_repo(repo: impl Into<PathBuf>, path: impl Into<PathBuf>) -> Self {
        Self {
            repo: Some(repo.into()),
            path: path.into(),
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TmpWorktreeGuard {
    fn drop(&mut self) {
        if let Some(repo) = &self.repo {
            let _ = Command::new("git")
                .args(["-C", &repo.to_string_lossy()])
                .args([
                    "worktree",
                    "remove",
                    "--force",
                    &self.path.to_string_lossy(),
                ])
                .status();
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Write an executable file without opening the destination path for writing in the test process.
/// This prevents leaking a write descriptor to concurrently forked child processes, avoiding ETXTBSY.
pub(crate) fn write_executable(path: &Path, content: &str) {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    if !parent.exists() {
        std::fs::create_dir_all(parent).expect("create parent directory for executable");
    }
    let mut temp = tempfile::Builder::new()
        .prefix(".tmp-exec-")
        .tempfile_in(parent)
        .expect("create sibling temp file");
    use std::io::Write;
    temp.write_all(content.as_bytes()).expect("write executable content");
    temp.flush().expect("flush executable content");
    let status = Command::new("install")
        .args(["-m", "755"])
        .arg(temp.path())
        .arg(path)
        .status()
        .expect("run install");
    assert!(status.success(), "install -m 755 failed with status {status}");
    let _ = temp.close();
}

#[test]
fn write_executable_creates_runnable_file_without_etxtbsy() {
    let temp = tempfile::tempdir().expect("tempdir");
    let script = temp.path().join("sub/test-exec.sh");
    write_executable(&script, "#!/bin/sh\nprintf 'hello executable'\n");
    let output = Command::new(&script).output().expect("execute written file");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "hello executable");
}

