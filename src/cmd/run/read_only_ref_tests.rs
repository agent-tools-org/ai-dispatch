// Read-only ref attribution regressions through the settlement boundary.
// Uses real sibling salvage commits, recovery refs and fetched branch/tag refs.
// Deps: parent enforcement, isolated Store and Git filesystem fixtures.

use super::*;
use crate::types::TaskStatus;
use std::{fs, process::Command};

#[path = "read_only_concurrent_tests.rs"]
mod concurrent_tests;

use crate::support::test_env::git;

fn fixture() -> (tempfile::TempDir, Store, TaskId, Vec<String>) {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.name", "Test"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["commit", "--allow-empty", "-m", "base"]);
    let store = Store::open_memory().unwrap();
    let id = TaskId("t-ref-diagnostic".into());
    let task = task(&id);
    store.insert_task(&task).unwrap();
    let before = capture_baseline(&store, &id, repo.path().to_str())
        .unwrap()
        .unwrap();
    (repo, store, id, before)
}

fn diagnostic(repo: &Path, store: &Store, id: &TaskId, before: &[String]) {
    let args = RunArgs {
        read_only: true,
        ..Default::default()
    };
    assert!(matches!(
        settle(store, id, &args, repo.to_str().unwrap(), Some(before)).unwrap(),
        DirtyWorktreeAction::Continue
    ));
    assert_eq!(
        store.get_task(id.as_str()).unwrap().unwrap().status,
        TaskStatus::Done
    );
    let events = store.get_events(id.as_str()).unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.detail.contains("ref") && event.event_kind != EventKind::Error)
    );
}

#[test]
fn fix_round_one_read_only_sibling_salvage_is_diagnostic() {
    let (repo, store, id, before) = fixture();
    let sibling = tempfile::tempdir().unwrap();
    git(
        repo.path(),
        &[
            "worktree",
            "add",
            "-b",
            "sibling",
            sibling.path().to_str().unwrap(),
        ],
    );
    fs::write(sibling.path().join("salvaged.txt"), "sibling work\n").unwrap();
    git(sibling.path(), &["add", "."]);
    git(sibling.path(), &["commit", "-m", "salvage"]);
    diagnostic(repo.path(), &store, &id, &before);
}

#[test]
fn fix_round_one_read_only_recovery_ref_is_diagnostic() {
    let (repo, store, id, before) = fixture();
    git(repo.path(), &["update-ref", "refs/aid/recovery/sibling", "HEAD"]);
    diagnostic(repo.path(), &store, &id, &before);
}

#[test]
fn fix_round_one_read_only_fetch_and_tags_are_diagnostic() {
    let (repo, store, id, before) = fixture();
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "--bare"]);
    git(
        repo.path(),
        &[
            "push",
            remote.path().to_str().unwrap(),
            "HEAD:refs/heads/main",
            "HEAD:refs/tags/v1",
        ],
    );
    git(
        repo.path(),
        &[
            "fetch",
            remote.path().to_str().unwrap(),
            "refs/heads/main:refs/remotes/origin/main",
            "refs/tags/v1:refs/tags/v1",
        ],
    );
    diagnostic(repo.path(), &store, &id, &before);
}

#[test]
fn fix_round_one_read_only_packed_ref_rewrite_preserves_logical_state() {
    let (repo, store, id, before) = fixture();
    git(repo.path(), &["pack-refs", "--all", "--prune"]);
    let args = RunArgs {
        read_only: true,
        ..Default::default()
    };
    assert!(matches!(
        settle(&store, &id, &args, repo.path().to_str().unwrap(), Some(&before)).unwrap(),
        DirtyWorktreeAction::Continue
    ));
    assert!(store.latest_error(id.as_str()).is_none());
}

#[test]
fn fix_round_one_read_only_own_branch_and_head_changes_remain_violations() {
    let (repo, store, id, before) = fixture();
    git(repo.path(), &["commit", "--allow-empty", "-m", "forbidden"]);
    let args = RunArgs {
        read_only: true,
        ..Default::default()
    };
    assert!(matches!(
        settle(&store, &id, &args, repo.path().to_str().unwrap(), Some(&before)).unwrap(),
        DirtyWorktreeAction::Failed
    ));
    let error = store.latest_error(id.as_str()).unwrap();
    assert!(error.contains(".git/HEAD"));
    assert!(error.contains(".git/refs/heads/main"));
}

fn task(id: &TaskId) -> crate::types::Task {
    crate::types::Task {
        agent: crate::types::AgentKind::Codex,
        prompt: "audit".into(),
        exit_code: Some(0),
        verify_status: crate::types::VerifyStatus::Skipped,
        read_only: true,
        ..crate::types::Task::test_fixture(id.clone(), TaskStatus::Done)
    }
}
