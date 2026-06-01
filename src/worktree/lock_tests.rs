// Worktree lock tests for atomic acquisition and cleanup.
// Exports: none.
// Deps: super worktree helpers, tempfile, std threading primitives.

use super::{
    check_worktree_lock, clear_worktree_lock, create_worktree, try_acquire_worktree_lock,
};
use super::path::WorktreeHomeGuard;
use super::state::write_worktree_lock;
use crate::test_subprocess;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Barrier};
use tempfile::TempDir;

fn git(repo_dir: &Path, args: &[&str]) {
    assert!(Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy()])
        .args(args)
        .status()
        .expect("git command should run")
        .success());
}

fn init_repo(repo_dir: &Path) {
    git(repo_dir, &["init", "-b", "main"]);
    git(repo_dir, &["config", "user.email", "test@example.com"]);
    git(repo_dir, &["config", "user.name", "Test User"]);
    std::fs::write(repo_dir.join("file.txt"), "hello\n").expect("fixture file should write");
    git(repo_dir, &["add", "file.txt"]);
    git(repo_dir, &["commit", "-m", "init"]);
}

#[test]
fn try_acquire_worktree_lock_rejects_existing_and_recovers_stale_lock() {
    let dir = TempDir::new().expect("tempdir should be created");

    assert!(try_acquire_worktree_lock(dir.path(), "t-first").is_ok());
    let err = try_acquire_worktree_lock(dir.path(), "t-second")
        .expect_err("second live acquisition should fail");
    assert_eq!(err, "t-first");

    clear_worktree_lock(dir.path());
    write_worktree_lock(dir.path(), "t-stale");
    std::fs::write(dir.path().join(".aid-lock"), "task=t-stale\npid=999999999\n")
        .expect("stale lock should write");

    assert!(try_acquire_worktree_lock(dir.path(), "t-after-stale").is_ok());
}

#[test]
fn write_worktree_lock_rekeys_owner_to_new_task_id() {
    // Mirrors the AutoSuffix path: the lock is acquired with the pre-suffix ID,
    // then re-keyed to the suffixed ID after conflict resolution.
    let dir = TempDir::new().expect("tempdir should be created");

    try_acquire_worktree_lock(dir.path(), "t-ebcf").expect("lock should be acquired");
    assert_eq!(check_worktree_lock(dir.path()).as_deref(), Some("t-ebcf"));

    write_worktree_lock(dir.path(), "t-ebcf-2");
    assert_eq!(check_worktree_lock(dir.path()).as_deref(), Some("t-ebcf-2"));
}

#[test]
fn try_acquire_worktree_lock_recovers_empty_legacy_lock() {
    let dir = TempDir::new().expect("tempdir should be created");
    std::fs::write(dir.path().join(".aid-lock"), "").expect("empty lock should write");

    // Empty locks cannot be produced by atomic acquisition, so treat them as stale.
    assert!(try_acquire_worktree_lock(dir.path(), "t-new").is_ok());
}

#[test]
fn try_acquire_worktree_lock_malformed_cleanup_allows_one_winner() {
    for attempt in 0..5 {
        let dir = TempDir::new().expect("tempdir should be created");
        std::fs::write(dir.path().join(".aid-lock"), "").expect("empty lock should write");
        let start = Arc::new(Barrier::new(2));
        let thread_dir = dir.path().to_path_buf();
        let thread_start = Arc::clone(&start);

        let p1 = std::thread::spawn(move || {
            thread_start.wait();
            try_acquire_worktree_lock(&thread_dir, "P1")
        });
        start.wait();
        let p2_result = try_acquire_worktree_lock(dir.path(), "P2");
        let p1_result = p1.join().expect("thread should join");
        let results = [("P1", p1_result), ("P2", p2_result)];
        let winners: Vec<&str> = results
            .iter()
            .filter_map(|(task_id, result)| result.is_ok().then_some(*task_id))
            .collect();

        assert_eq!(winners.len(), 1, "attempt {attempt}: expected one winner");
        let winner = winners[0];
        for (task_id, result) in results {
            if task_id != winner {
                let err = result.expect_err("loser should see winner holder");
                assert!(
                    err.contains(winner),
                    "attempt {attempt}: loser error {err:?} should mention {winner}"
                );
            }
        }
    }
}

#[test]
fn clear_worktree_lock_sweeps_orphan_temp_files() {
    let dir = TempDir::new().expect("tempdir should be created");
    let lock = dir.path().join(".aid-lock");
    let tmp = dir.path().join(".aid-lock.tmp.foo");
    let malformed = dir.path().join(".aid-lock.malformed.foo");
    std::fs::write(&lock, "task=t-lock\npid=999999999\n").expect("lock should write");
    std::fs::write(&tmp, "tmp").expect("tmp lock should write");
    std::fs::write(&malformed, "malformed").expect("malformed lock should write");

    clear_worktree_lock(dir.path());

    assert!(!lock.exists());
    assert!(!tmp.exists());
    assert!(!malformed.exists());
}

#[test]
fn create_worktree_refuses_existing_locked_worktree() {
    let _permit = test_subprocess::acquire();
    let home = TempDir::new().expect("home tempdir should be created");
    let _home_guard = WorktreeHomeGuard::set(home.path());
    let repo = TempDir::new().expect("repo tempdir should be created");
    init_repo(repo.path());

    let branch = "feat/locked-reuse";
    let info = create_worktree(repo.path(), branch, None).expect("worktree should be created");
    try_acquire_worktree_lock(&info.path, "t-holder").expect("lock should be acquired");

    let err = create_worktree(repo.path(), branch, None).expect_err("locked worktree should fail");

    assert!(err.to_string().contains("locked by task t-holder"));
}
