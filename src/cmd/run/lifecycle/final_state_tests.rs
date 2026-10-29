// Tests for lifecycle final-state capture before failed worktree cleanup.
// Covers final branch persistence when fast-fail cleanup removes the worktree.
// Deps: run lifecycle, Store, git CLI, worktree creation.

use super::{
    run_lifecycle::{post_run_lifecycle, LifecycleMode},
    run_prompt::PromptBundle,
    RunArgs,
};
use crate::store::Store;
use crate::support::test_subprocess;
use crate::types::{AgentKind, Task, TaskId, TaskStatus};
use chrono::Local;
use std::path::Path;

use std::sync::Arc;

use crate::support::test_env::git;

fn init_repo() -> tempfile::TempDir {
    crate::support::test_repo::repo_with_file("base.txt", "base\n", "base")
}

fn failed_task(id: &str, repo: &Path, wt: &Path, branch: &str) -> Task {
    Task {
        repo_path: Some(repo.display().to_string()),
        worktree_path: Some(wt.display().to_string()),
        worktree_branch: Some(branch.to_string()),
        duration_ms: Some(100),
        exit_code: Some(1),
        created_at: Local::now(),
        completed_at: Some(Local::now()),
        ..crate::types::Task::test_fixture(TaskId(id.to_string()), TaskStatus::Failed)
    }
}

#[tokio::test]
async fn failed_lifecycle_captures_final_branch_before_fast_fail_cleanup() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = "fix/failure-final-capture";
    let info = crate::workspace::worktree::create_worktree(repo.path(), branch, None).unwrap();
    git(&info.path, &["switch", "-c", "agent/failure-final"]);
    std::fs::write(info.path.join("final.txt"), "final\n").unwrap();
    git(&info.path, &["add", "final.txt"]);
    git(&info.path, &["commit", "-m", "final work"]);
    let store = Arc::new(Store::open_memory().unwrap());
    let task = failed_task("t-failure-final-capture", repo.path(), &info.path, branch);
    store.insert_task(&task).unwrap();
    let prompt_bundle = PromptBundle {
        effective_prompt: "prompt".to_string(),
        ..Default::default()
    };

    post_run_lifecycle(
        LifecycleMode::Background,
        &store,
        &task.id,
        &RunArgs::default(),
        AgentKind::Codex,
        "codex",
        Some(&info.path.display().to_string()),
        Some(&repo.path().display().to_string()),
        Some(&info.path.display().to_string()),
        None,
        &[],
        &prompt_bundle,
        TaskStatus::Running,
        None,
    )
    .await
    .unwrap();

    let loaded = store.get_task(task.id.as_str()).unwrap().unwrap();
    assert_eq!(loaded.final_branch.as_deref(), Some("agent/failure-final"));
    assert!(loaded.final_head_sha.is_some());
    assert!(info.path.exists());
}
