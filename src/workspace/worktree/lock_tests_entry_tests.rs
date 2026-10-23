// Production entry point regression tests split to keep the parent within its size limit.
// Deps: parent test fixtures and production APIs.

use super::*;

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

    assert!(
        lock.exists(),
        "malformed lock must be left for acquisition-path recovery"
    );
}

#[test]
fn clear_worktree_lock_refuses_wrong_task_id() {
    let dir = TempDir::new().expect("tempdir should be created");
    try_acquire_worktree_lock_with_store(dir.path(), "t-owner", None).expect("lock should be acquired");

    let err = clear_worktree_lock(dir.path(), "t-other").expect_err("wrong task should fail");

    assert_eq!(err, "t-owner");
    assert_eq!(
        check_worktree_lock_with_store(dir.path(), None).as_deref(),
        Some("t-owner")
    );
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
