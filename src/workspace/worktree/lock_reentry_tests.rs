// Owner-chain re-entry tests for nested worktree leases.
// Covers descendant allow and unrelated refuse on the same lock.
// Deps: worktree lock helpers, Store, tempfile.

use super::{
    check_worktree_lock, try_acquire_worktree_lock_with_store,
};
use crate::store::Store;
use crate::types::{Task, TaskId, TaskStatus};

use tempfile::TempDir;

fn make_task(id: &str, status: TaskStatus, parent: Option<&str>) -> Task {
    Task {
        prompt: "test".to_string(),
        parent_task_id: parent.map(str::to_string),
        ..crate::types::Task::test_fixture(TaskId(id.to_string()), status)
    }
}

#[test]
fn descendant_reenters_ancestor_held_worktree_unrelated_is_refused() {
    let store = Store::open_memory().expect("store should open");
    store
        .insert_task(&make_task("t-ancestor", TaskStatus::Running, None))
        .expect("ancestor");
    store
        .insert_task(&make_task("t-child", TaskStatus::Pending, Some("t-ancestor")))
        .expect("child");
    store
        .insert_task(&make_task("t-unrelated", TaskStatus::Pending, None))
        .expect("unrelated");

    let dir = TempDir::new().expect("tempdir should be created");
    try_acquire_worktree_lock_with_store(dir.path(), "t-ancestor", Some(&store))
        .expect("ancestor acquires");

    try_acquire_worktree_lock_with_store(dir.path(), "t-child", Some(&store))
        .expect("descendant must re-enter ancestor lease");
    assert_eq!(
        check_worktree_lock(dir.path()).as_deref(),
        Some("t-ancestor"),
        "re-entry must leave the ancestor lease in place"
    );

    let err = try_acquire_worktree_lock_with_store(dir.path(), "t-unrelated", Some(&store))
        .expect_err("unrelated task must still be refused");
    assert_eq!(err, "t-ancestor");
    println!(
        "PROOF re-entry: descendant t-child entered worktree held by t-ancestor; lock holder remains t-ancestor"
    );
    println!("PROOF refuse: unrelated t-unrelated refused with holder={err}");
}
