// Tests for Rust build-cache environment setup.
// Exports: none. Deps: env helpers, subprocess env guards, tempfile.

use super::*;
use std::ffi::{OsStr, OsString};
use std::process::Command;
use std::sync::{Mutex, MutexGuard, OnceLock};

struct EnvVarGuard {
    key: &'static str,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
        let previous = std::env::var_os(key);
        unsafe { std::env::set_var(key, value) };
        Self { key, previous }
    }

    fn remove(key: &'static str) -> Self {
        let previous = std::env::var_os(key);
        unsafe { std::env::remove_var(key) };
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => unsafe { std::env::set_var(self.key, value) },
            None => unsafe { std::env::remove_var(self.key) },
        }
    }
}

#[test]
fn target_dir_for_worktree_seeds_missing_branch_target_from_base() {
    let _env = env_lock();
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().join("target");
    let debug = base.join("debug");
    std::fs::create_dir_all(&debug).unwrap();
    std::fs::write(debug.join("artifact.txt"), "cached").unwrap();
    let _target_dir = EnvVarGuard::set("CARGO_TARGET_DIR", &base);

    let branch_target = target_dir_for_worktree(Some("feat/shared-cache")).unwrap();

    assert_eq!(
        branch_target,
        base.join("feat-shared-cache").to_string_lossy()
    );
    assert_eq!(
        std::fs::read_to_string(base.join("feat-shared-cache/debug/artifact.txt")).unwrap(),
        "cached"
    );
}

#[test]
fn apply_rust_build_cache_env_sets_target_and_sccache_when_available() {
    let _env = env_lock();
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let bin = temp.path().join("bin");
    let target = temp.path().join("target");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(project.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
    write_executable(bin.join("sccache"));
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    let _path = EnvVarGuard::set("PATH", path);
    let _target_dir = EnvVarGuard::set("CARGO_TARGET_DIR", &target);
    let _wrapper = EnvVarGuard::remove("RUSTC_WRAPPER");
    let mut cmd = Command::new("echo");

    apply_rust_build_cache_env(&mut cmd, project.to_str(), None);

    assert_eq!(command_env(&cmd, "CARGO_TARGET_DIR").as_deref(), Some(target.to_string_lossy().as_ref()));
    assert_eq!(command_env(&cmd, "RUSTC_WRAPPER").as_deref(), Some("sccache"));
}

#[test]
fn apply_rust_build_cache_env_keeps_command_rustc_wrapper() {
    let _env = env_lock();
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
    let _wrapper = EnvVarGuard::remove("RUSTC_WRAPPER");
    let mut cmd = Command::new("echo");
    cmd.env("RUSTC_WRAPPER", "custom-wrapper");

    apply_rust_build_cache_env(&mut cmd, project.to_str(), None);

    assert_eq!(command_env(&cmd, "RUSTC_WRAPPER").as_deref(), Some("custom-wrapper"));
}

#[test]
fn apply_rust_build_cache_env_keeps_ambient_rustc_wrapper() {
    let _env = env_lock();
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
    let _wrapper = EnvVarGuard::set("RUSTC_WRAPPER", "ambient-wrapper");
    let mut cmd = Command::new("echo");

    apply_rust_build_cache_env(&mut cmd, project.to_str(), None);

    assert!(command_env(&cmd, "RUSTC_WRAPPER").is_none());
}

fn command_env(cmd: &Command, name: &str) -> Option<String> {
    cmd.get_envs()
        .find(|(key, _)| *key == name)
        .and_then(|(_, value)| value)
        .map(|value| value.to_string_lossy().to_string())
}

fn write_executable(path: std::path::PathBuf) {
    std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).unwrap();
    }
}

fn env_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}
