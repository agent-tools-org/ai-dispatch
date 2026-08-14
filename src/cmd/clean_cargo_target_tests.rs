// Tests for task-owned Cargo target and fallback cleanup.
// Covers terminal ownership, live-worktree protection, and unattributed paths.
// Deps: clean_cargo_target, Store, tempfile, rusqlite.

use super::*;
use crate::test_env::CargoTargetDirGuard;
use crate::store::Store;
use rusqlite::params;
use std::fs;
use std::path::Path;

fn insert_task(
    store: &Store,
    id: &str,
    status: &str,
    repo_path: Option<&Path>,
    worktree_path: Option<&Path>,
    branch: Option<&str>,
) {
    store
        .db()
        .execute(
            "INSERT INTO tasks (id, agent, prompt, status, repo_path, worktree_path, worktree_branch, created_at)
             VALUES (?1, 'codex', 'cleanup test', ?2, ?3, ?4, ?5, '2026-01-01T00:00:00Z')",
            params![
                id,
                status,
                repo_path.map(|path| path.to_string_lossy().into_owned()),
                worktree_path.map(|path| path.to_string_lossy().into_owned()),
                branch,
            ],
        )
        .unwrap();
}

fn init_repo(repo: &Path) {
    std::process::Command::new("git")
        .args(["init", "-b", "main", &repo.to_string_lossy()])
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "config", "user.email", "test@example.com"])
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "config", "user.name", "Test User"])
        .status()
        .unwrap();
    fs::write(repo.join("file.txt"), "base\n").unwrap();
    std::process::Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "add", "file.txt"])
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "commit", "-m", "base"])
        .status()
        .unwrap();
}

#[test]
fn cleanup_removes_only_terminal_task_owned_targets() {
    let _permit = crate::test_subprocess::acquire();
    let aid_home = tempfile::tempdir().unwrap();
    let _aid_guard = crate::paths::AidHomeGuard::set(aid_home.path());
    let target_root = aid_home.path().join("cargo-target");
    let _target_guard = CargoTargetDirGuard::set(&target_root);
    let fallback_root = tempfile::tempdir().unwrap();
    let store = Store::open_memory().unwrap();

    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path());
    let live_worktree = tempfile::tempdir().unwrap();
    let live_branch = "feat/live-clean-target";
    let live_path = live_worktree.path().join("live");
    std::process::Command::new("git")
        .args([
            "-C",
            &repo.path().to_string_lossy(),
            "worktree",
            "add",
            &live_path.to_string_lossy(),
            "-b",
            live_branch,
        ])
        .status()
        .unwrap();

    let stale_branch = "feat/stale-clean-target";
    let stale_worktree = repo.path().join("missing-worktree");
    insert_task(
        &store,
        "t-stale",
        "done",
        Some(repo.path()),
        Some(&stale_worktree),
        Some(stale_branch),
    );
    insert_task(
        &store,
        "t-live",
        "done",
        Some(repo.path()),
        Some(&live_path),
        Some(live_branch),
    );
    insert_task(&store, "t-blank-branch", "done", None, None, Some(""));

    let stale_target = target_root.join(crate::agent::env::branch_target_name(stale_branch));
    let live_target = target_root.join(crate::agent::env::branch_target_name(live_branch));
    let stale_fallback = fallback_root
        .path()
        .join(crate::cmd::build::build_fallback::cwd_key(&stale_worktree));
    let unattributed = target_root.join("not-in-task-record");
    for path in [&stale_target, &live_target, &stale_fallback, &unattributed] {
        fs::create_dir_all(path).unwrap();
        fs::write(path.join("artifact"), vec![b'x'; 8]).unwrap();
    }
    fs::write(target_root.join("root-artifact"), b"root").unwrap();

    let mut sizes = crate::cmd::clean_size::SizeTracker::new();
    clean_orphaned_branch_targets(&store, false, Some(fallback_root.path()), &mut sizes).unwrap();

    assert!(!stale_target.exists());
    assert!(!stale_fallback.exists());
    assert!(live_target.exists());
    assert!(unattributed.exists());
    assert!(target_root.exists());
}

#[test]
fn cleanup_preserves_root_when_branch_name_matches_root_basename() {
    let _permit = crate::test_subprocess::acquire();
    let aid_home = tempfile::tempdir().unwrap();
    let _aid_guard = crate::paths::AidHomeGuard::set(aid_home.path());
    let target_root = aid_home.path().join("cargo-target");
    let _target_guard = CargoTargetDirGuard::set(&target_root);
    let fallback_root = tempfile::tempdir().unwrap();
    let store = Store::open_memory().unwrap();
    let live_worktree = tempfile::tempdir().unwrap();

    insert_task(
        &store,
        "t-root-name",
        "done",
        None,
        None,
        Some("cargo-target"),
    );
    insert_task(
        &store,
        "t-live-leaf",
        "done",
        None,
        Some(live_worktree.path()),
        Some("live-leaf"),
    );

    let live_target = target_root.join("live-leaf");
    fs::create_dir_all(&live_target).unwrap();
    fs::write(target_root.join("root-artifact"), b"root").unwrap();

    let mut sizes = crate::cmd::clean_size::SizeTracker::new();
    clean_orphaned_branch_targets(&store, false, Some(fallback_root.path()), &mut sizes).unwrap();

    assert!(target_root.exists());
    assert!(live_target.exists());
}

#[test]
fn hint_scan_reports_lower_bound_when_entry_limit_truncates_scan() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("first"), vec![b'a'; 7]).unwrap();
    fs::write(temp.path().join("second"), vec![b'b'; 11]).unwrap();
    fs::write(temp.path().join("third"), vec![b'c'; 13]).unwrap();

    let expected = crate::cmd::clean_size::get_dir_size(temp.path()).unwrap();
    let mut sizes = crate::cmd::clean_size::SizeTracker::new();
    let measured = scan_hint_path(&mut sizes, temp.path(), 10_000, 2).unwrap();

    assert!(measured.truncated);
    assert_eq!(measured.entries, 2);
    assert!(measured.bytes < expected);
}

#[test]
fn hint_scan_marks_complete_scan_when_limits_are_not_reached() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("first"), vec![b'a'; 7]).unwrap();
    fs::write(temp.path().join("second"), vec![b'b'; 11]).unwrap();

    let expected = crate::cmd::clean_size::get_dir_size(temp.path()).unwrap();
    let mut sizes = crate::cmd::clean_size::SizeTracker::new();
    let measured = scan_hint_path(&mut sizes, temp.path(), 10_000, 3).unwrap();

    assert!(!measured.truncated);
    assert_eq!(measured.bytes, expected);
}

#[test]
fn hint_estimate_survives_entry_limit_before_byte_threshold() {
    let _permit = crate::test_subprocess::acquire();
    let aid_home = tempfile::tempdir().unwrap();
    let _aid_guard = crate::paths::AidHomeGuard::set(aid_home.path());
    let target_root = aid_home.path().join("cargo-target");
    let _target_guard = CargoTargetDirGuard::set(&target_root);
    let store = Store::open_memory().unwrap();
    let branch = "feat/truncated-hint";
    insert_task(&store, "t-truncated-hint", "done", None, None, Some(branch));

    let target = target_root.join(crate::agent::env::branch_target_name(branch));
    fs::create_dir_all(&target).unwrap();
    for index in 0..=2 {
        fs::File::create(target.join(format!("artifact-{index}"))).unwrap();
    }

    let estimate = has_reclaimable_space_above_threshold_with_entry_limit(&store, 2)
        .unwrap()
        .expect("entry-budget truncation must remain visible");

    assert!(estimate.truncated);
    assert!(estimate.bytes < crate::cmd::clean_size::CLEANUP_HINT_THRESHOLD_BYTES);
}

#[test]
fn cleanup_reclaims_orphan_fallback_targets_and_protects_running_tasks() {
    let store = Store::open_memory().unwrap();
    let fallback_root = tempfile::tempdir().unwrap();

    let running_wt = Path::new("/tmp/running-wt");
    insert_task(
        &store,
        "t-running",
        "running",
        None,
        Some(running_wt),
        None,
    );

    let running_fallback = fallback_root
        .path()
        .join(crate::cmd::build::build_fallback::cwd_key(running_wt));
    let orphan_fallback = fallback_root.path().join("orphan-fallback-12345");

    fs::create_dir_all(&running_fallback).unwrap();
    fs::write(running_fallback.join("file"), b"running").unwrap();
    fs::create_dir_all(&orphan_fallback).unwrap();
    fs::write(orphan_fallback.join("file"), b"orphan").unwrap();

    let mut sizes = crate::cmd::clean_size::SizeTracker::new();
    clean_orphaned_branch_targets(&store, false, Some(fallback_root.path()), &mut sizes).unwrap();

    assert!(running_fallback.exists(), "Running task fallback dir must be protected");
    assert!(!orphan_fallback.exists(), "Orphan fallback dir must be cleaned");
}

#[test]
fn cleanup_refuses_symlinks_under_fallback_root() {
    let store = Store::open_memory().unwrap();
    let fallback_root = tempfile::tempdir().unwrap();
    let external_dir = tempfile::tempdir().unwrap();
    let external_file = external_dir.path().join("external-file");
    fs::write(&external_file, b"important").unwrap();

    let symlink_path = fallback_root.path().join("symlink-target");
    #[cfg(unix)]
    std::os::unix::fs::symlink(external_dir.path(), &symlink_path).unwrap();

    let mut sizes = crate::cmd::clean_size::SizeTracker::new();
    clean_orphaned_branch_targets(&store, false, Some(fallback_root.path()), &mut sizes).unwrap();

    assert!(external_file.exists(), "External file through symlink must not be touched");
}
