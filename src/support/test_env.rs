// Shared test environment guards, owning home fixtures, and Git command helpers.
// Exports: environment/cleanup guards, TestHome, Git output, and executable helpers.
// Deps: tempfile, isolated path overrides, std process/filesystem/synchronization.

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
        Self {
            _lock: lock,
            previous,
        }
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
        Self {
            _lock: lock,
            previous,
        }
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
    temp.write_all(content.as_bytes())
        .expect("write executable content");
    temp.flush().expect("flush executable content");
    let status = Command::new("install")
        .args(["-m", "755"])
        .arg(temp.path())
        .arg(path)
        .status()
        .expect("run install");
    assert!(
        status.success(),
        "install -m 755 failed with status {status}"
    );
    let _ = temp.close();
}

#[test]
fn write_executable_creates_runnable_file_without_etxtbsy() {
    let temp = tempfile::tempdir().expect("tempdir");
    let script = temp.path().join("sub/test-exec.sh");
    write_executable(&script, "#!/bin/sh\nprintf 'hello executable'\n");
    let output = Command::new(&script)
        .output()
        .expect("execute written file");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "hello executable");
}

pub(crate) fn git(dir: &Path, args: &[&str]) {
    git_stdout(dir, args);
}

pub(crate) fn git_output(dir: &Path, args: &[&str]) -> String {
    git_stdout(dir, args).trim().to_string()
}

pub(crate) fn git_stdout(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("git stdout")
}

/// Own the temporary directory and its thread-local override for the same lifetime.
pub(crate) struct TestHome {
    _guard: crate::support::paths::AidHomeGuard,
    dir: tempfile::TempDir,
}

impl TestHome {
    pub(crate) fn new() -> Self {
        let dir = tempfile::tempdir().expect("test home");
        let guard = crate::support::paths::AidHomeGuard::set(dir.path());
        Self { _guard: guard, dir }
    }

    pub(crate) fn with_dirs() -> Self {
        let home = Self::new();
        crate::support::paths::ensure_dirs().expect("aid directories");
        home
    }
}

impl std::ops::Deref for TestHome {
    type Target = tempfile::TempDir;

    fn deref(&self) -> &Self::Target {
        &self.dir
    }
}

#[test]
fn nested_test_home_restores_outer_home_and_removes_its_directory() {
    let outer = TestHome::new();
    let inner_path = {
        let inner = TestHome::with_dirs();
        assert_eq!(crate::support::paths::aid_dir(), inner.path());
        assert!(inner.path().join("logs").is_dir());
        assert!(inner.path().join("jobs").is_dir());
        inner.path().to_path_buf()
    };
    assert_eq!(crate::support::paths::aid_dir(), outer.path());
    assert!(!inner_path.exists());
}

#[test]
fn test_home_restores_outer_home_after_unwinding() {
    let outer = TestHome::new();
    let result = std::panic::catch_unwind(|| {
        let _inner = TestHome::new();
        panic!("test fixture unwind");
    });
    assert!(result.is_err());
    assert_eq!(crate::support::paths::aid_dir(), outer.path());
}

#[test]
fn git_stdout_preserves_porcelain_whitespace() {
    let _permit = crate::support::test_subprocess::acquire();
    let repo = tempfile::tempdir().expect("repo");
    git(repo.path(), &["init", "-b", "main"]);
    std::fs::write(repo.path().join("tracked.txt"), "base").expect("base file");
    git(repo.path(), &["add", "."]);
    git(
        repo.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-m",
            "base",
        ],
    );
    std::fs::write(repo.path().join("tracked.txt"), "modified").expect("modified file");
    assert_eq!(
        git_stdout(repo.path(), &["status", "--porcelain"]),
        " M tracked.txt\n"
    );
    assert_eq!(
        git_output(repo.path(), &["status", "--porcelain"]),
        "M tracked.txt"
    );
}
