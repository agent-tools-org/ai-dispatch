// Tests for batch retry logic.
// Exports: (tests only)
// Deps: super::shared + batch_retry
use super::shared::make_stored_task;
use crate::store::Store;
use crate::types::{AgentKind, TaskStatus};
use std::sync::Arc;

use super::super::batch_retry::{retry_failed, retry_task_to_run_args};

#[test]
fn retry_task_to_run_args_uses_parent_and_original_fields() {
    let mut task = make_stored_task("t-1234", AgentKind::Codex, TaskStatus::Failed);
    task.prompt = "retry me".to_string();
    task.repo_path = Some("/tmp/repo".to_string());
    task.worktree_branch = Some("feat/retry".to_string());
    task.output_path = Some("out.txt".to_string());
    task.model = Some("o3".to_string());
    task.verify = Some("cargo check".to_string());
    task.read_only = true;
    task.budget = true;

    let run_args = retry_task_to_run_args(&task, "wg-batch", Some("cursor"));

    assert_eq!(run_args.agent_name, "cursor");
    assert_eq!(run_args.prompt, "retry me");
    assert_eq!(run_args.repo, Some("/tmp/repo".to_string()));
    assert_eq!(run_args.worktree, Some("feat/retry".to_string()));
    assert_eq!(run_args.verify, Some("cargo check".to_string()));
    assert_eq!(run_args.parent_task_id, Some("t-1234".to_string()));
    assert_eq!(run_args.group, Some("wg-batch".to_string()));
    assert!(run_args.background);
    assert!(run_args.read_only);
    assert!(run_args.budget);
}

#[test]
fn retry_task_to_run_args_prefers_existing_worktree_path() {
    let temp = tempfile::tempdir().unwrap();
    let mut task = make_stored_task("t-1234", AgentKind::Codex, TaskStatus::Failed);
    task.worktree_path = Some(temp.path().display().to_string());
    task.worktree_branch = Some("feat/retry".to_string());

    let run_args = retry_task_to_run_args(&task, "wg-batch", None);

    assert_eq!(run_args.dir, task.worktree_path);
    assert_eq!(run_args.worktree, None);
}

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
