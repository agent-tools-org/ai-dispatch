// Tests for the `run` command module after splitting from run.rs.
// Covers dispatch validation, quota detection, cascade behavior, and dry-run flow.
// Depends on the parent run module, store, paths, tokio, and tempfile.
use super::*;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskStatus, VerifyStatus};

use std::sync::Arc;
use tempfile::TempDir;

use crate::support::test_env::git;

fn make_failed_task(task_id: &str) -> Task {
    Task {
        exit_code: Some(1),
        verify_status: VerifyStatus::Failed,
        ..crate::types::Task::test_fixture(TaskId(task_id.to_string()), TaskStatus::Failed)
    }
}

#[path = "delivery_recovery_tests.rs"]
mod delivery_recovery_tests;

#[path = "transcript_tests.rs"]
mod run_transcript_tests;

#[path = "async_tests.rs"]
mod run_async_tests;

fn quota_worktree(dir: &std::path::Path, branch: Option<&str>) -> std::path::PathBuf {
    let wt_dir = dir.join("wt");
    std::fs::create_dir_all(&wt_dir).unwrap();
    if let Some(branch) = branch {
        git(&wt_dir, &["init", "-b", branch]);
    } else {
        git(&wt_dir, &["init"]);
    }
    git(&wt_dir, &["config", "user.email", "aid@example.com"]);
    git(&wt_dir, &["config", "user.name", "Aid Tester"]);
    std::fs::write(wt_dir.join("file.txt"), "initial").unwrap();
    git(&wt_dir, &["add", "file.txt"]);
    git(&wt_dir, &["commit", "-m", "initial"]);
    wt_dir
}

fn assert_terminal_failure_preserved(store: &Store, id: &TaskId, quota: Option<&str>) {
    assert!(quota.is_some(), "quota fixture must be recognized");
    let before = store.get_task(id.as_str()).unwrap().unwrap();
    let events = store.get_events(id.as_str()).unwrap().len();
    assert!(!store.update_task_status(id.as_str(), TaskStatus::Done).unwrap());
    let after = store.get_task(id.as_str()).unwrap().unwrap();
    assert_eq!(after.status, TaskStatus::Failed);
    assert_eq!(after.completed_at, before.completed_at);
    assert_eq!(after.duration_ms, before.duration_ms);
    assert_eq!(store.get_events(id.as_str()).unwrap().len(), events);
}

#[path = "tests_quota.rs"]
mod quota;

#[path = "tests_validation.rs"]
mod validation;

#[path = "tests_lifecycle.rs"]
mod lifecycle;
