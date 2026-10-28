// Tests for merge source branch resolution after agents switch branches.
// Covers final_branch preference, drift confirmation, and real-branch merging.
// Deps: parent merge module helpers, Store, git CLI.

use super::*;
use crate::store::Store;
use crate::support::test_subprocess;


use std::path::Path;

use tempfile::TempDir;

use crate::support::test_env::git;

fn init_repo() -> TempDir {
    let repo = TempDir::new().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@aid.dev"]);
    git(repo.path(), &["config", "user.name", "Test"]);
    std::fs::write(repo.path().join("base.txt"), "base\n").unwrap();
    git(repo.path(), &["add", "base.txt"]);
    git(repo.path(), &["commit", "-m", "base"]);
    repo
}

fn task(repo: &Path, dispatch_branch: &str, final_branch: &str) -> Task {
    Task {
        prompt: "test".to_string(),
        repo_path: Some(repo.to_string_lossy().to_string()),
        worktree_branch: Some(dispatch_branch.to_string()),
        final_branch: Some(final_branch.to_string()),
        ..crate::types::Task::test_fixture(TaskId("t-final-merge".to_string()), TaskStatus::Done)
    }
}

#[test]
fn drifted_merge_requires_force_then_merges_final_branch() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    git(repo.path(), &["branch", "dispatch/branch"]);
    git(repo.path(), &["switch", "-c", "agent/final"]);
    std::fs::write(repo.path().join("final.txt"), "final\n").unwrap();
    git(repo.path(), &["add", "final.txt"]);
    git(repo.path(), &["commit", "-m", "final work"]);
    git(repo.path(), &["switch", "main"]);
    let store = Store::open_memory().unwrap();
    let task = task(repo.path(), "dispatch/branch", "agent/final");
    store.insert_task(&task).unwrap();

    let blocked = merge_single_with_output(&store, task.id.as_str(), false, false, false, None, true);
    assert!(blocked.is_err());
    assert!(!repo.path().join("final.txt").exists());

    merge_single_with_output(&store, task.id.as_str(), false, false, true, None, true).unwrap();

    assert!(repo.path().join("final.txt").exists());
    let loaded = store.get_task(task.id.as_str()).unwrap().unwrap();
    assert_eq!(loaded.status, TaskStatus::Merged);
}
