// Production entry point regression tests split to keep the parent within its size limit.
// Deps: parent test fixtures and production APIs.

use super::*;

#[test]
fn rescue_amends_when_start_sha_differs_from_head() {
    let _permit = test_subprocess::acquire();
    let dir = repo();
    commit_path(dir.path(), "tracked.txt", "tracked");
    let start_sha = head(dir.path());

    // Simulate agent committing something
    commit_path(dir.path(), "agent.txt", "agent");
    let count_before = commit_count(dir.path());
    let before = head(dir.path());

    // Agent left dirty files
    write_path(dir.path(), "src/amended.rs", "pub fn amended() {}\n");

    let outcome =
        rescue_dirty_worktree_with_baseline(dir.path().to_str().unwrap(), "task-123", None, Some(&start_sha))
            .unwrap();

    assert!(outcome.committed);
    assert_ne!(head(dir.path()), before);
    // count remains same because of amend
    assert_eq!(commit_count(dir.path()), count_before);
}
