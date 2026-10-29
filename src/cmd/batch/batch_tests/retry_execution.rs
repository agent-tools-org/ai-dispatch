// Tests for batch retry filtering, serial execution, and repository fixtures.
// Exports: (tests only)
// Deps: super::shared + batch_retry
use super::shared::make_stored_task;
use crate::store::Store;
use crate::support::paths::AidHomeGuard;
use crate::types::{AgentKind, TaskStatus};
use std::path::{PathBuf};

use std::sync::Arc;

use super::super::batch_retry::retry_failed;

#[tokio::test]
async fn retry_failed_returns_ok_when_no_failed_tasks_exist() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = make_stored_task("t-1234", AgentKind::Codex, TaskStatus::Done);
    task.workgroup_id = Some("wg-batch".to_string());
    store.insert_task(&task).unwrap();

    let result = retry_failed(store, "wg-batch", None, false).await;

    assert!(result.is_ok());
}

#[test]
fn retry_filter_includes_waiting_only_when_requested() {
    use super::super::batch_retry::should_retry_task;

    assert!(should_retry_task(TaskStatus::Failed, false));
    assert!(should_retry_task(TaskStatus::Skipped, false));
    assert!(!should_retry_task(TaskStatus::Waiting, false));
    assert!(should_retry_task(TaskStatus::Waiting, true));
    assert!(!should_retry_task(TaskStatus::Running, true));
}

#[tokio::test]
async fn serial_retry_continues_after_a_failed_task() {
    let first = make_stored_task("t-first", AgentKind::Codex, TaskStatus::Failed);
    let second = make_stored_task("t-second", AgentKind::Codex, TaskStatus::Failed);
    let attempted = Arc::new(std::sync::Mutex::new(Vec::new()));
    let attempted_by_retry = attempted.clone();

    let failures =
        super::super::batch_retry::retry_tasks_serially(vec![first, second], move |task| {
            let attempted = attempted_by_retry.clone();
            async move {
                attempted.lock().unwrap().push(task.id.to_string());
                if task.id.as_str() == "t-first" {
                    anyhow::bail!("simulated retry failure");
                }
                Ok(())
            }
        })
        .await;

    assert_eq!(
        attempted.lock().unwrap().as_slice(),
        ["t-first", "t-second"]
    );
    assert_eq!(failures, vec!["t-first"]);
}

pub(super) fn isolated_home() -> AidHomeGuard {
    let temp = tempfile::tempdir().unwrap();
    AidHomeGuard::set(temp.path())
}

pub(super) use crate::support::test_env::git;

pub(super) fn init_repo() -> tempfile::TempDir {
    crate::support::test_repo::repo_with_file("file.txt", "hello\n", "init")
}

pub(super) fn linked_worktree(branch: &str) -> (tempfile::TempDir, tempfile::TempDir, PathBuf) {
    let repo = init_repo();
    let linked_root = tempfile::tempdir().unwrap();
    let linked = linked_root.path().join("linked");
    git(
        repo.path(),
        &["worktree", "add", "-b", branch, &linked.to_string_lossy()],
    );
    (repo, linked_root, linked)
}
