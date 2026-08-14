// Tests for task-owned Cargo target and fallback cleanup.
// Covers terminal ownership, live-worktree protection, and cwd existence checks.
// Deps: clean_cargo_target, Store, tempfile, rusqlite.

use super::*;
use crate::test_env::CargoTargetDirGuard;
use crate::store::Store;
use rusqlite::params;
use std::fs;
use std::path::Path;

fn insert_task(s: &Store, id: &str, status: &str, repo: Option<&Path>, wt: Option<&Path>, branch: Option<&str>) {
    s.db().execute(
        "INSERT INTO tasks (id, agent, prompt, status, repo_path, worktree_path, worktree_branch, created_at)
         VALUES (?1, 'codex', 'test', ?2, ?3, ?4, ?5, '2026-01-01T00:00:00Z')",
        params![id, status, repo.map(|p| p.to_string_lossy().into_owned()), wt.map(|p| p.to_string_lossy().into_owned()), branch],
    ).unwrap();
}

fn init_repo(repo: &Path) {
    let r = &repo.to_string_lossy();
    std::process::Command::new("git").args(["init", "-b", "main", r]).status().unwrap();
    std::process::Command::new("git").args(["-C", r, "config", "user.email", "t@e.co"]).status().unwrap();
    std::process::Command::new("git").args(["-C", r, "config", "user.name", "T"]).status().unwrap();
    fs::write(repo.join("file.txt"), "base\n").unwrap();
    std::process::Command::new("git").args(["-C", r, "add", "file.txt"]).status().unwrap();
    std::process::Command::new("git").args(["-C", r, "commit", "-m", "base"]).status().unwrap();
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
        .args(["-C", &repo.path().to_string_lossy(), "worktree", "add", &live_path.to_string_lossy(), "-b", live_branch])
        .status()
        .unwrap();

    let stale_branch = "feat/stale-clean-target";
    let stale_worktree = repo.path().join("missing-worktree");
    insert_task(&store, "t-stale", "done", Some(repo.path()), Some(&stale_worktree), Some(stale_branch));
    insert_task(&store, "t-live", "done", Some(repo.path()), Some(&live_path), Some(live_branch));
    insert_task(&store, "t-blank-branch", "done", None, None, Some(""));

    let stale_target = target_root.join(crate::agent::env::branch_target_name(stale_branch));
    let live_target = target_root.join(crate::agent::env::branch_target_name(live_branch));
    let stale_fallback = fallback_root.path().join(crate::cmd::build::build_fallback::cwd_key(&stale_worktree));
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

    insert_task(&store, "t-root-name", "done", None, None, Some("cargo-target"));
    insert_task(&store, "t-live-leaf", "done", None, Some(live_worktree.path()), Some("live-leaf"));

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
fn cleanup_reclaims_task_fallback_targets_and_protects_running_tasks() {
    let store = Store::open_memory().unwrap();
    let fallback_root = tempfile::tempdir().unwrap();

    let stale_wt = Path::new("/tmp/stale-fallback-wt");
    let running_wt = Path::new("/tmp/running-fallback-wt");
    insert_task(&store, "t-stale", "done", None, Some(stale_wt), None);
    insert_task(&store, "t-running", "running", None, Some(running_wt), None);

    let stale_fallback = fallback_root.path().join(crate::cmd::build::build_fallback::cwd_key(stale_wt));
    let running_fallback = fallback_root.path().join(crate::cmd::build::build_fallback::cwd_key(running_wt));
    let unattributed_fallback = fallback_root.path().join("unattributed-fallback-12345");

    fs::create_dir_all(&stale_fallback).unwrap();
    fs::create_dir_all(&running_fallback).unwrap();
    fs::create_dir_all(&unattributed_fallback).unwrap();

    let mut sizes = crate::cmd::clean_size::SizeTracker::new();
    clean_orphaned_branch_targets(&store, false, Some(fallback_root.path()), &mut sizes).unwrap();

    assert!(!stale_fallback.exists(), "Terminal task fallback dir must be reclaimed");
    assert!(running_fallback.exists(), "Running task fallback dir must be protected");
    assert!(unattributed_fallback.exists(), "Unattributed fallback dir must be preserved");
}

#[test]
fn safety_guard_parent_must_be_fallback_root() {
    let fallback_root = tempfile::tempdir().unwrap();
    let nested_dir = fallback_root.path().join("subdir");
    let deep_target = nested_dir.join("deep_target");
    fs::create_dir_all(&deep_target).unwrap();

    assert!(
        !is_safe_fallback_target_for_removal(&deep_target, fallback_root.path()),
        "Path whose parent is not fallback_root must be rejected"
    );
}

#[test]
fn safety_guard_refuses_symlinks_under_fallback_root() {
    let fallback_root = tempfile::tempdir().unwrap();
    let external_dir = tempfile::tempdir().unwrap();
    let external_file = external_dir.path().join("external-file");
    fs::write(&external_file, b"important").unwrap();

    let symlink_path = fallback_root.path().join("symlink-target");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(external_dir.path(), &symlink_path).unwrap();

        assert!(
            !is_safe_fallback_target_for_removal(&symlink_path, fallback_root.path()),
            "Symlink under fallback root must be rejected"
        );

        let store = Store::open_memory().unwrap();
        let wt_path = Path::new("/tmp/stale-wt-symlink");
        insert_task(&store, "t-symlink", "done", None, Some(wt_path), None);
        let target_for_wt = fallback_root.path().join(crate::cmd::build::build_fallback::cwd_key(wt_path));
        let _ = fs::remove_dir_all(&target_for_wt);
        std::os::unix::fs::symlink(external_dir.path(), &target_for_wt).unwrap();

        let mut sizes = crate::cmd::clean_size::SizeTracker::new();
        clean_orphaned_branch_targets(&store, false, Some(fallback_root.path()), &mut sizes).unwrap();

        assert!(fs::symlink_metadata(&target_for_wt).is_ok(), "Symlink node itself must survive cleanup");
        assert!(external_file.exists(), "External file through symlink must survive cleanup");
    }
}

#[test]
fn cleanup_skips_fallback_target_when_cwd_still_exists_on_disk() {
    let store = Store::open_memory().unwrap();
    let fallback_root = tempfile::tempdir().unwrap();
    let live_dir = tempfile::tempdir().unwrap();
    let stale_wt = Path::new("/tmp/nonexistent-cwd-wt-12345");

    insert_task(&store, "t-live-cwd", "done", None, Some(live_dir.path()), None);
    insert_task(&store, "t-stale-cwd", "done", None, Some(stale_wt), None);

    let live_fallback = fallback_root.path().join(crate::cmd::build::build_fallback::cwd_key(live_dir.path()));
    let stale_fallback = fallback_root.path().join(crate::cmd::build::build_fallback::cwd_key(stale_wt));
    fs::create_dir_all(&live_fallback).unwrap();
    fs::create_dir_all(&stale_fallback).unwrap();

    let mut sizes = crate::cmd::clean_size::SizeTracker::new();
    clean_orphaned_branch_targets(&store, false, Some(fallback_root.path()), &mut sizes).unwrap();

    assert!(live_fallback.exists(), "Fallback target whose cwd exists must be preserved");
    assert!(!stale_fallback.exists(), "Fallback target whose cwd no longer exists must be deleted");
}

#[test]
fn remove_task_fallback_preserves_repo_and_removes_deleted_worktree() {
    let store = Store::open_memory().unwrap();
    let fallback_root_dir = tempfile::tempdir().unwrap();
    let repo_dir = tempfile::tempdir().unwrap();
    let stale_wt = Path::new("/tmp/nonexistent-wt-for-task-fallback");

    insert_task(&store, "t-task-gc", "done", Some(repo_dir.path()), Some(stale_wt), None);
    let task = store.get_task("t-task-gc").unwrap().unwrap();

    let repo_fallback = fallback_root_dir.path().join(crate::cmd::build::build_fallback::cwd_key(repo_dir.path()));
    let wt_fallback = fallback_root_dir.path().join(crate::cmd::build::build_fallback::cwd_key(stale_wt));
    fs::create_dir_all(&repo_fallback).unwrap();
    fs::create_dir_all(&wt_fallback).unwrap();

    let _fallback_guard = crate::test_env::FallbackTargetDirGuard::set(fallback_root_dir.path());
    let res = remove_task_fallback_target_dirs(&store, &task);

    res.unwrap();
    assert!(repo_fallback.exists(), "Repo fallback target must persist forever");
    assert!(!wt_fallback.exists(), "Deleted worktree fallback target must be removed");
}

#[test]
fn cwd_existence_check_fails_closed_on_stat_error() {
    let temp = tempfile::tempdir().unwrap();
    let existing_path = temp.path();
    let missing_path = temp.path().join("missing-dir-12345");
    let empty_path = Path::new("");

    assert!(!cwd_no_longer_exists(existing_path), "Existing directory must report false");
    assert!(cwd_no_longer_exists(&missing_path), "Missing directory must report true");
    assert!(!cwd_no_longer_exists(empty_path), "Empty path must fail closed and report false");
}
