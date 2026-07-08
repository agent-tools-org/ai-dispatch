// Worktree lock tests for atomic acquisition and cleanup.
// Exports: none.
// Deps: super worktree helpers, tempfile, std threading primitives.

use super::{
    check_worktree_lock, check_worktree_lock_with_store, clear_worktree_lock, create_worktree,
    rekey_worktree_lock_to_worker, simulate_stale_recovery_race, try_acquire_worktree_lock,
    try_acquire_worktree_lock_with_store,
};
use super::path::WorktreeHomeGuard;
use super::write_worktree_lock;
use crate::store::Store;
use crate::test_subprocess;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
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

    clear_worktree_lock(dir.path(), "t-first").expect("lock should clear");
    write_worktree_lock(dir.path(), "t-stale");
    std::fs::write(
        dir.path().join(".aid-lock"),
        "version=1\ntask_id=t-stale\nowner_pid=999999999\nworker_pid=999999998\n",
    )
    .expect("stale lock should write");

    assert!(try_acquire_worktree_lock(dir.path(), "t-after-stale").is_ok());
}

#[test]
fn storeless_check_treats_dead_owner_without_worker_as_held() {
    // Re-key window: launcher exited, worker has not re-keyed yet. Without a
    // store there is no way to rule the lease out, so it must stay held and
    // the check must not touch the file.
    let dir = TempDir::new().expect("tempdir should be created");
    let lock = dir.path().join(".aid-lock");
    std::fs::write(&lock, "version=1\ntask_id=t-window\nowner_pid=999999999\n")
        .expect("lock should write");

    assert_eq!(check_worktree_lock(dir.path()).as_deref(), Some("t-window"));
    assert!(lock.exists());

    let err = try_acquire_worktree_lock(dir.path(), "t-thief")
        .expect_err("rekey-window lock must refuse store-less acquisition");
    assert_eq!(err, "t-window");
    assert!(lock.exists());
}

#[test]
fn store_terminal_status_releases_dead_owner_lock() {
    let store = Store::open_memory().expect("store should open");
    store
        .insert_task(&make_task("t-done", TaskStatus::Done))
        .expect("task should insert");
    let dir = TempDir::new().expect("tempdir should be created");
    let lock = dir.path().join(".aid-lock");
    std::fs::write(&lock, "version=1\ntask_id=t-done\nowner_pid=999999999\n")
        .expect("lock should write");

    assert!(check_worktree_lock_with_store(dir.path(), Some(&store)).is_none());
    assert!(lock.exists(), "check must be side-effect-free");
    assert!(try_acquire_worktree_lock_with_store(dir.path(), "t-next", Some(&store)).is_ok());
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
fn worker_pid_keeps_background_lock_live_when_launcher_pid_is_dead() {
    let dir = TempDir::new().expect("tempdir should be created");
    let content = format!(
        "version=1\ntask_id=t-bg\nowner_pid=999999999\nworker_pid={}\n",
        std::process::id()
    );
    std::fs::write(dir.path().join(".aid-lock"), content).expect("lock should write");

    assert_eq!(check_worktree_lock(dir.path()).as_deref(), Some("t-bg"));
    assert!(dir.path().join(".aid-lock").exists());
}

#[test]
fn rekey_worktree_lock_to_worker_refuses_wrong_task() {
    let dir = TempDir::new().expect("tempdir should be created");
    try_acquire_worktree_lock(dir.path(), "t-owner").expect("lock should be acquired");

    let err = rekey_worktree_lock_to_worker(dir.path(), "t-other", std::process::id())
        .expect_err("wrong task should not rekey");

    assert_eq!(err, "t-owner");
    assert_eq!(check_worktree_lock(dir.path()).as_deref(), Some("t-owner"));
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
fn stale_lock_recovery_does_not_clobber_concurrent_fresh_lock() {
    let dir = TempDir::new().expect("tempdir should be created");
    std::fs::write(dir.path().join(".aid-lock"), "").expect("empty lock should write");
    let competitor_path = dir.path().to_path_buf();

    let result = simulate_stale_recovery_race(dir.path(), "P1", || {}, || {
        try_acquire_worktree_lock(&competitor_path, "P2").expect("competitor should acquire");
    });

    assert_eq!(result.expect_err("recovery should lose the race"), "P2");
    assert_eq!(check_worktree_lock(dir.path()).as_deref(), Some("P2"));
}

#[test]
fn stale_lock_recovery_loses_when_competitor_recovers_before_rename() {
    // Dangerous ordering: the competitor completes a full independent
    // stale-recovery + acquisition BEFORE the holder's rename, so the
    // holder's rename captures the competitor's fresh lock. The holder must
    // detect the mismatch, restore the fresh lock, and lose the race.
    let dir = TempDir::new().expect("tempdir should be created");
    std::fs::write(
        dir.path().join(".aid-lock"),
        "version=1\ntask_id=t-stale\nowner_pid=999999999\nworker_pid=999999998\n",
    )
    .expect("stale lock should write");
    let competitor_path = dir.path().to_path_buf();

    let result = simulate_stale_recovery_race(
        dir.path(),
        "P1",
        || {
            try_acquire_worktree_lock(&competitor_path, "P2").expect("competitor should acquire");
        },
        || {},
    );

    assert_eq!(result.expect_err("holder must lose after competitor recovery"), "P2");
    assert_eq!(check_worktree_lock(dir.path()).as_deref(), Some("P2"));
}

#[test]
fn clear_worktree_lock_sweeps_orphan_temp_files() {
    let dir = TempDir::new().expect("tempdir should be created");
    let lock = dir.path().join(".aid-lock");
    let tmp = dir.path().join(".aid-lock.tmp.foo");
    let malformed = dir.path().join(".aid-lock.malformed.foo");
    std::fs::write(&lock, "version=1\ntask_id=t-lock\nowner_pid=999999999\n").expect("lock should write");
    std::fs::write(&tmp, "tmp").expect("tmp lock should write");
    std::fs::write(&malformed, "malformed").expect("malformed lock should write");

    clear_worktree_lock(dir.path(), "t-lock").expect("lock should clear");

    assert!(!lock.exists());
    assert!(!tmp.exists());
    assert!(!malformed.exists());
}

#[test]
fn clear_worktree_lock_leaves_malformed_record_for_acquisition() {
    let dir = TempDir::new().expect("tempdir should be created");
    let lock = dir.path().join(".aid-lock");
    std::fs::write(&lock, "not-a-lock-record\n").expect("malformed lock should write");

    clear_worktree_lock(dir.path(), "t-any").expect("clear should succeed");

    assert!(lock.exists(), "malformed lock must be left for acquisition-path recovery");
}

#[test]
fn clear_worktree_lock_refuses_wrong_task_id() {
    let dir = TempDir::new().expect("tempdir should be created");
    try_acquire_worktree_lock(dir.path(), "t-owner").expect("lock should be acquired");

    let err = clear_worktree_lock(dir.path(), "t-other").expect_err("wrong task should fail");

    assert_eq!(err, "t-owner");
    assert_eq!(check_worktree_lock(dir.path()).as_deref(), Some("t-owner"));
}

#[test]
fn missing_pid_lock_uses_store_status_before_stale_cleanup() {
    let store = Store::open_memory().expect("store should open");
    store
        .insert_task(&make_task("t-running", TaskStatus::Running))
        .expect("task should insert");
    let dir = TempDir::new().expect("tempdir should be created");
    std::fs::write(dir.path().join(".aid-lock"), "version=1\ntask_id=t-running\n")
        .expect("lock should write");

    assert_eq!(
        check_worktree_lock_with_store(dir.path(), Some(&store)).as_deref(),
        Some("t-running")
    );
    assert!(dir.path().join(".aid-lock").exists());
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

fn make_task(id: &str, status: TaskStatus) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "test".to_string(),
        resolved_prompt: None,
        category: None,
        status,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        worktree_path: None,
        worktree_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        model: None,
        cost_usd: None,
        exit_code: None,
        created_at: Local::now(),
        completed_at: None,
        verify: None,
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
    }
}
