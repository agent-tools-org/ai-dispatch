// Tests for Rust build-cache environment setup.
// Exports: none. Deps: env helpers, subprocess env guards, tempfile.

use super::*;
use crate::test_env::CargoTargetDirGuard;
use std::process::Command;
use std::os::unix::fs::PermissionsExt;

#[test]
fn target_dir_for_worktree_keeps_source_and_branches_as_siblings() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("target");
    let source = root.join("_base");
    let _target_dir = CargoTargetDirGuard::set(&root);

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
    let _clone = super::super::cargo_target::CloneSeedGuard::regular_copy();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("target");
    let source = root.join("_base");
    let existing_branch = root.join("existing-branch");
    let debug = source.join("debug");
    std::fs::create_dir_all(&debug).unwrap();
    std::fs::create_dir_all(&existing_branch).unwrap();
    std::fs::write(debug.join("artifact.txt"), "cached").unwrap();
    let _target_dir = CargoTargetDirGuard::set(&root);

    let outcome = seed_branch_target_dir(None, "feat/shared-cache").unwrap();

    assert!(matches!(outcome, BranchTargetSeedOutcome::Seeded { .. }));
    assert_eq!(
        std::fs::read_to_string(root.join("feat-shared-cache/debug/artifact.txt")).unwrap(),
        "cached"
    );
    assert!(!root.join("feat-shared-cache/existing-branch").exists());
}

#[test]
fn seed_branch_target_dir_falls_back_to_root_artifact_entries() {
    let clone_guard = super::super::cargo_target::CloneSeedGuard::regular_copy();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("target");
    let debug = root.join("debug");
    let release = root.join("release");
    let existing_branch = root.join("existing-branch");
    std::fs::create_dir_all(&debug).unwrap();
    std::fs::create_dir_all(&release).unwrap();
    std::fs::create_dir_all(&existing_branch).unwrap();
    std::fs::write(debug.join("artifact.txt"), "debug").unwrap();
    std::fs::write(release.join("artifact.txt"), "release").unwrap();
    std::fs::write(root.join(".rustc_info.json"), "{}").unwrap();
    let target_dir_guard = CargoTargetDirGuard::set(&root);

    let outcome = seed_branch_target_dir(None, "feat/shared-cache").unwrap();

    assert!(matches!(outcome, BranchTargetSeedOutcome::Seeded { .. }));
    assert_eq!(
        std::fs::read_to_string(root.join("feat-shared-cache/debug/artifact.txt")).unwrap(),
        "debug"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("feat-shared-cache/release/artifact.txt")).unwrap(),
        "release"
    );
    assert!(root.join("feat-shared-cache/.rustc_info.json").is_file());
    assert!(!root.join("feat-shared-cache/existing-branch").exists());
    drop((target_dir_guard, clone_guard));
}

#[test]
fn seed_branch_target_dir_prefers_base_over_root_artifact_entries() {
    let clone_guard = super::super::cargo_target::CloneSeedGuard::regular_copy();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("target");
    let base_debug = root.join("_base/debug");
    let root_debug = root.join("debug");
    std::fs::create_dir_all(&base_debug).unwrap();
    std::fs::create_dir_all(&root_debug).unwrap();
    std::fs::create_dir_all(root.join("release")).unwrap();
    std::fs::write(base_debug.join("artifact.txt"), "base").unwrap();
    std::fs::write(root_debug.join("artifact.txt"), "fallback").unwrap();
    let target_dir_guard = CargoTargetDirGuard::set(&root);

    let outcome = seed_branch_target_dir(None, "feat/shared-cache").unwrap();

    assert!(matches!(outcome, BranchTargetSeedOutcome::Seeded { .. }));
    assert_eq!(
        std::fs::read_to_string(root.join("feat-shared-cache/debug/artifact.txt")).unwrap(),
        "base"
    );
    assert!(!root.join("feat-shared-cache/release").exists());
    drop((target_dir_guard, clone_guard));
}

#[test]
fn seed_branch_target_dir_creates_empty_target_when_seed_is_skipped() {
    let clone_guard = super::super::cargo_target::CloneSeedGuard::regular_copy();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("target");
    let target_dir_guard = CargoTargetDirGuard::set(&root);

    let outcome = seed_branch_target_dir(None, "feat/shared-cache").unwrap();

    assert_eq!(
        outcome,
        BranchTargetSeedOutcome::Skipped {
            target: root.join("feat-shared-cache").to_string_lossy().into_owned(),
            reason: "base target directory is missing and no root cargo artifacts are present"
                .to_string(),
        }
    );
    assert!(root.join("feat-shared-cache").is_dir());
    drop((target_dir_guard, clone_guard));
}

#[test]
fn seed_branch_target_dir_keeps_multiple_branch_targets_flat() {
    let _clone = super::super::cargo_target::CloneSeedGuard::regular_copy();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("target");
    let source = root.join("_base/debug");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("artifact.txt"), "cached").unwrap();
    let _target_dir = CargoTargetDirGuard::set(&root);

    for branch in ["feat/cache-a", "feat/cache-b", "feat/cache-c"] {
        let outcome = seed_branch_target_dir(None, branch).unwrap();
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
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("some/root/project");
    let escaped = temp.path().join("some/root/fix-foo");
    let _target_dir = CargoTargetDirGuard::set(&root);

    let branch_target = PathBuf::from(target_dir_for_worktree(Some("fix/foo")).unwrap());
    let source_target = PathBuf::from(target_dir_for_worktree(None).unwrap());

    assert!(branch_target.starts_with(&root));
    assert_eq!(branch_target, root.join("fix-foo"));
    assert_ne!(branch_target, escaped);
    assert_eq!(source_target, root.join("_base"));
}

#[test]
fn explicit_cargo_target_dir_namespaces_same_branch_per_project() {
    let temp = tempfile::tempdir().unwrap();
    let first_project = temp.path().join("root/project-a");
    let second_project = temp.path().join("root/project-b");

    let first_target = {
        let _target_dir = CargoTargetDirGuard::set(&first_project);
        target_dir_for_worktree(Some("fix/shared")).unwrap()
    };
    let second_target = {
        let _target_dir = CargoTargetDirGuard::set(&second_project);
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
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let target = temp.path().join("target");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
    let _target_dir = CargoTargetDirGuard::set(&target);
    let mut cmd = Command::new("echo");

    apply_rust_build_cache_env(&mut cmd, project.to_str(), None);

    let expected = target.join("_base").to_string_lossy().to_string();
    assert_eq!(
        command_env(&cmd, "CARGO_TARGET_DIR").as_deref(),
        Some(expected.as_str())
    );
    assert!(command_env(&cmd, "RUSTC_WRAPPER").is_none());
}

#[test]
fn rust_build_cache_target_dir_requires_a_rust_project() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target");
    let _target_dir = CargoTargetDirGuard::set(&target);

    assert_eq!(rust_build_cache_target_dir(Some(temp.path().to_str().unwrap()), None), None);
}

#[test]
fn apply_codex_home_env_uses_durable_real_home() {
    let mut cmd = Command::new("echo");

    apply_codex_home_env(&mut cmd).unwrap();

    let codex_home = command_env(&cmd, "CODEX_HOME").unwrap();
    assert_eq!(
        PathBuf::from(codex_home),
        super::super::home_isolation::resolve_real_home().unwrap().join(".codex")
    );
}

#[test]
fn apply_run_env_sets_host_toolchain_paths() {
    let aid_home = tempfile::tempdir().unwrap();
    let _aid_guard = crate::paths::AidHomeGuard::set(aid_home.path());
    let mut cmd = Command::new("echo");
    let opts = RunOpts {
        dir: None,
        output: None,
        result_file: None,
        model: None,
        budget: false,
        read_only: false,
        sandbox: false,
        context_files: vec![],
        session_id: None,
        env: None,
        env_forward: None,
    };

    let guard = apply_run_env(&mut cmd, &opts, None).unwrap();
    let real_home = super::super::home_isolation::resolve_real_home().unwrap();
    assert_eq!(command_env(&cmd, "CARGO_HOME"), Some(real_home.join(".cargo").to_string_lossy().into_owned()));
    assert_eq!(command_env(&cmd, "RUSTUP_HOME"), Some(real_home.join(".rustup").to_string_lossy().into_owned()));
    drop(guard);
}

#[test]
fn executable_search_requires_execute_permission() {
    let temp = tempfile::tempdir().unwrap();
    let binary = temp.path().join("agent");
    std::fs::write(&binary, "#!/bin/sh\n").unwrap();
    assert!(!executable_in_paths("agent", vec![temp.path().to_path_buf()]));

    let mut permissions = std::fs::metadata(&binary).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&binary, permissions).unwrap();
    assert!(executable_in_paths("agent", vec![temp.path().to_path_buf()]));
}

#[test]
fn durable_codex_home_is_disabled_for_isolated_wrappers() {
    assert!(should_use_durable_codex_home(AgentKind::Codex, false, false));
    assert!(!should_use_durable_codex_home(AgentKind::Codex, true, false));
    assert!(!should_use_durable_codex_home(AgentKind::Codex, false, true));
    assert!(!should_use_durable_codex_home(AgentKind::Claude, false, false));
}

fn command_env(cmd: &Command, name: &str) -> Option<String> {
    cmd.get_envs()
        .find(|(key, _)| *key == name)
        .and_then(|(_, value)| value)
        .map(|value| value.to_string_lossy().to_string())
}
