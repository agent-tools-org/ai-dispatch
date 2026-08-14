// Tests for task-owned Cargo target and fallback cleanup.
// Covers terminal ownership, live-worktree protection, and liveness verification.
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
        fs::write(path.join(".cargo-lock"), b"").unwrap();
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
    fs::write(stale_fallback.join(".cargo-lock"), b"").unwrap();
    fs::create_dir_all(&running_fallback).unwrap();
    fs::write(running_fallback.join(".cargo-lock"), b"").unwrap();
    fs::create_dir_all(&unattributed_fallback).unwrap();
    fs::write(unattributed_fallback.join(".cargo-lock"), b"").unwrap();

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
#[cfg(unix)]
fn cleanup_skips_live_target_with_held_cargo_lock() {
    let store = Store::open_memory().unwrap();
    let fallback_root = tempfile::tempdir().unwrap();
    let wt_path = Path::new("/tmp/stale-wt-locked");
    insert_task(&store, "t-stale-locked", "done", None, Some(wt_path), None);

    let target = fallback_root.path().join(crate::cmd::build::build_fallback::cwd_key(wt_path));
    fs::create_dir_all(&target).unwrap();
    let lock_file = target.join(".cargo-lock");
    fs::write(&lock_file, b"").unwrap();

    use std::os::unix::io::AsRawFd;
    let lock_holder = fs::File::open(&lock_file).unwrap();
    let ret = unsafe { libc::flock(lock_holder.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    assert_eq!(ret, 0, "Failed to take lock in test setup");

    let mut sizes = crate::cmd::clean_size::SizeTracker::new();
    clean_orphaned_branch_targets(&store, false, Some(fallback_root.path()), &mut sizes).unwrap();

    assert!(target.exists(), "Target with held .cargo-lock must NOT be deleted");

    unsafe { libc::flock(lock_holder.as_raw_fd(), libc::LOCK_UN) };

    clean_orphaned_branch_targets(&store, false, Some(fallback_root.path()), &mut sizes).unwrap();
    assert!(!target.exists(), "Target with released .cargo-lock must be deleted");
}

#[test]
fn cleanup_skips_recently_modified_target_without_cargo_lock() {
    let store = Store::open_memory().unwrap();
    let fallback_root = tempfile::tempdir().unwrap();
    let wt_path = Path::new("/tmp/stale-wt-recent");
    insert_task(&store, "t-stale-recent", "done", None, Some(wt_path), None);

    let target = fallback_root.path().join(crate::cmd::build::build_fallback::cwd_key(wt_path));
    fs::create_dir_all(&target).unwrap();
    let artifact = target.join("build-artifact");
    fs::write(&artifact, b"recent").unwrap();

    let mut sizes = crate::cmd::clean_size::SizeTracker::new();
    clean_orphaned_branch_targets(&store, false, Some(fallback_root.path()), &mut sizes).unwrap();

    assert!(target.exists(), "Recently modified target without .cargo-lock must NOT be deleted");

    let old_time = SystemTime::now() - Duration::from_secs(60);
    fs::File::open(&artifact).unwrap().set_modified(old_time).unwrap();
    fs::File::open(&target).unwrap().set_modified(old_time).unwrap();

    clean_orphaned_branch_targets(&store, false, Some(fallback_root.path()), &mut sizes).unwrap();
    assert!(!target.exists(), "Aged target without .cargo-lock must be deleted");
}
