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
fn target_dir_for_worktree_keeps_source_and_branches_as_siblings() {
    let _env = env_lock();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("target");
    let source = root.join("_base");
    let _target_dir = EnvVarGuard::set("CARGO_TARGET_DIR", &root);

    let branch_target = target_dir_for_worktree(Some("feat/shared-cache")).unwrap();
    let source_target = target_dir_for_worktree(None).unwrap();

    assert_eq!(
        branch_target,
        root.join("feat-shared-cache").to_string_lossy()
    );
    assert_eq!(
        source_target,
        source.to_string_lossy()
    );
    assert!(!std::path::Path::new(&branch_target).exists());
}

#[test]
fn seed_branch_target_dir_copies_base_without_existing_branches() {
    let _env = env_lock();
    let _clone = super::super::cargo_target::CloneSeedGuard::regular_copy();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("target");
    let source = root.join("_base");
    let existing_branch = root.join("existing-branch");
    let debug = source.join("debug");
    std::fs::create_dir_all(&debug).unwrap();
    std::fs::create_dir_all(&existing_branch).unwrap();
    std::fs::write(debug.join("artifact.txt"), "cached").unwrap();
    let _target_dir = EnvVarGuard::set("CARGO_TARGET_DIR", &root);

    let outcome = seed_branch_target_dir("feat/shared-cache").unwrap();

    assert!(matches!(outcome, BranchTargetSeedOutcome::Seeded { .. }));
    assert_eq!(
        std::fs::read_to_string(root.join("feat-shared-cache/debug/artifact.txt")).unwrap(),
        "cached"
    );
    assert!(!root.join("feat-shared-cache/existing-branch").exists());
}

#[test]
fn seed_branch_target_dir_keeps_multiple_branch_targets_flat() {
    let _env = env_lock();
    let _clone = super::super::cargo_target::CloneSeedGuard::regular_copy();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("target");
    let source = root.join("_base/debug");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("artifact.txt"), "cached").unwrap();
    let _target_dir = EnvVarGuard::set("CARGO_TARGET_DIR", &root);

    for branch in ["feat/cache-a", "feat/cache-b", "feat/cache-c"] {
        let outcome = seed_branch_target_dir(branch).unwrap();
        assert!(matches!(outcome, BranchTargetSeedOutcome::Seeded { .. }));
    }

    for target in ["feat-cache-a", "feat-cache-b", "feat-cache-c"] {
        assert!(root.join(target).join("debug/artifact.txt").is_file());
        for nested in ["feat-cache-a", "feat-cache-b", "feat-cache-c"] {
            assert!(!root.join(target).join(nested).exists());
        }
    }
}

#[test]
fn explicit_cargo_target_dir_keeps_branch_target_inside_project_root() {
    let _env = env_lock();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("some/root/project");
    let escaped = temp.path().join("some/root/fix-foo");
    let _target_dir = EnvVarGuard::set("CARGO_TARGET_DIR", &root);

    let branch_target = PathBuf::from(target_dir_for_worktree(Some("fix/foo")).unwrap());
    let source_target = PathBuf::from(target_dir_for_worktree(None).unwrap());

    assert!(branch_target.starts_with(&root));
    assert_eq!(branch_target, root.join("fix-foo"));
    assert_ne!(branch_target, escaped);
    assert_eq!(source_target, root.join("_base"));
}

#[test]
fn explicit_cargo_target_dir_namespaces_same_branch_per_project() {
    let _env = env_lock();
    let temp = tempfile::tempdir().unwrap();
    let first_project = temp.path().join("root/project-a");
    let second_project = temp.path().join("root/project-b");

    let first_target = {
        let _target_dir = EnvVarGuard::set("CARGO_TARGET_DIR", &first_project);
        target_dir_for_worktree(Some("fix/shared")).unwrap()
    };
    let second_target = {
        let _target_dir = EnvVarGuard::set("CARGO_TARGET_DIR", &second_project);
        target_dir_for_worktree(Some("fix/shared")).unwrap()
    };

    assert_eq!(
        first_target,
        first_project.join("fix-shared").to_string_lossy()
    );
    assert_eq!(
        second_target,
        second_project.join("fix-shared").to_string_lossy()
    );
    assert_ne!(first_target, second_target);
}

#[test]
fn apply_rust_build_cache_env_sets_target_only() {
    let _env = env_lock();
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let target = temp.path().join("target");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
    let _target_dir = EnvVarGuard::set("CARGO_TARGET_DIR", &target);
    let mut cmd = Command::new("echo");

    apply_rust_build_cache_env(&mut cmd, project.to_str(), None);

    let expected = target.join("_base").to_string_lossy().to_string();
    assert_eq!(
        command_env(&cmd, "CARGO_TARGET_DIR").as_deref(),
        Some(expected.as_str())
    );
    assert!(command_env(&cmd, "RUSTC_WRAPPER").is_none());
}

fn command_env(cmd: &Command, name: &str) -> Option<String> {
    cmd.get_envs()
        .find(|(key, _)| *key == name)
        .and_then(|(_, value)| value)
        .map(|value| value.to_string_lossy().to_string())
}

fn env_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}
