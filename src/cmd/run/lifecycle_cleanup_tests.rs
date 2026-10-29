// Tests for lifecycle ordering around completed worktree cleanup.
// Covers retry, normal completion, and failure preserving worktree artifacts.
// Isolates AID_HOME so post-run rate-limit cleanup never touches ~/.aid.
// Deps: post_run_lifecycle, Store, worktree helpers, git CLI, tempfile.

use super::{
    RunArgs,
    run_lifecycle::{LifecycleMode, post_run_lifecycle},
    run_prompt::PromptBundle,
};
use crate::{
    support::paths::{self, AidHomeGuard},
    store::Store,
    support::test_subprocess,
    types::{AgentKind, Task, TaskId, TaskStatus},
};
use chrono::Local;
use std::{path::Path, sync::Arc};

fn isolated_home() -> (tempfile::TempDir, AidHomeGuard) {
    let home = tempfile::tempdir().expect("home");
    let guard = AidHomeGuard::set(home.path());
    paths::ensure_dirs().expect("dirs");
    (home, guard)
}

use crate::support::test_env::git;

fn init_repo() -> tempfile::TempDir {
    crate::support::test_repo::repo_with_file("base.txt", "base\n", "base")
}

fn task(id: &str, repo: &Path, wt: &Path, branch: &str, output_path: &Path) -> Task {
    Task {
        repo_path: Some(repo.display().to_string()),
        worktree_path: Some(wt.display().to_string()),
        worktree_branch: Some(branch.to_string()),
        output_path: Some(output_path.display().to_string()),
        duration_ms: Some(1_000),
        created_at: Local::now(),
        completed_at: Some(Local::now()),
        ..crate::types::Task::test_fixture(TaskId(id.to_string()), TaskStatus::Done)
    }
}

fn prompt_bundle() -> PromptBundle {
    PromptBundle {
        effective_prompt: "prompt".to_string(),
        ..Default::default()
    }
}

fn committed_worktree(repo: &Path, branch: &str) -> std::path::PathBuf {
    let info = crate::workspace::worktree::create_worktree(repo, branch, None).unwrap();
    std::fs::write(info.path.join("done.txt"), "done\n").unwrap();
    git(&info.path, &["add", "done.txt"]);
    git(&info.path, &["commit", "-m", "done"]);
    info.path
}

async fn run_lifecycle(
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
) -> Option<TaskId> {
    post_run_lifecycle(
        LifecycleMode::Background,
        store,
        task_id,
        args,
        AgentKind::Codex,
        "codex",
        args.dir.as_ref(),
        args.repo.as_ref(),
        None,
        None,
        &[],
        &prompt_bundle(),
        store.get_task(task_id.as_str()).unwrap().unwrap().status,
        None,
    )
    .await
    .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn completed_worktree_survives_when_checklist_retry_dispatches() {
    let _permit = test_subprocess::acquire();
    let (_home, _guard) = isolated_home();
    let repo = init_repo();
    let branch = "fix/lifecycle-retry-keep";
    let wt = committed_worktree(repo.path(), branch);
    let output_path = wt.join("output.md");
    std::fs::write(&output_path, "alpha only\n").unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-cleanup-retry-keep".to_string());
    store
        .insert_task(&task(task_id.as_str(), repo.path(), &wt, branch, &output_path))
        .unwrap();
    let args = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "prompt".to_string(),
        repo: Some(repo.path().display().to_string()),
        dir: Some(wt.display().to_string()),
        checklist: vec!["beta".to_string()],
        retry: 1,
        dry_run: true,
        skills: vec![super::NO_SKILL_SENTINEL.to_string()],
        ..Default::default()
    };

    store.update_task_dispatch_args(task_id.as_str(), &args.dispatch_args_json().unwrap()).unwrap();
    let retry_id = run_lifecycle(&store, &task_id, &args).await;

    assert!(retry_id.is_some());
    assert!(wt.exists());
}

#[tokio::test(flavor = "current_thread")]
async fn completed_worktree_survives_when_no_retry_dispatches() {
    let _permit = test_subprocess::acquire();
    let (_home, _guard) = isolated_home();
    let repo = init_repo();
    let branch = "fix/lifecycle-finish-prune";
    let wt = committed_worktree(repo.path(), branch);
    let output_path = wt.join("output.md");
    std::fs::write(&output_path, "complete output\n").unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-cleanup-finish-prune".to_string());
    store
        .insert_task(&task(task_id.as_str(), repo.path(), &wt, branch, &output_path))
        .unwrap();
    let args = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "prompt".to_string(),
        repo: Some(repo.path().display().to_string()),
        dir: Some(wt.display().to_string()),
        dry_run: true,
        skills: vec![super::NO_SKILL_SENTINEL.to_string()],
        ..Default::default()
    };

    let retry_id = run_lifecycle(&store, &task_id, &args).await;

    assert!(retry_id.is_none());
    assert!(wt.exists());
}

#[tokio::test(flavor = "current_thread")]
async fn failed_lifecycle_retains_worktree_branch_and_output() {
    let _permit = test_subprocess::acquire();
    let (_home, _guard) = isolated_home();
    let repo = init_repo();
    let branch = "fix/lifecycle-failed-keep";
    let wt = committed_worktree(repo.path(), branch);
    let output_path = wt.join("output.md");
    std::fs::write(&output_path, "partial output\n").unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-failed-keep".to_string());
    let mut failed = task(task_id.as_str(), repo.path(), &wt, branch, &output_path);
    failed.status = TaskStatus::Failed;
    failed.duration_ms = Some(100);
    store.insert_task(&failed).unwrap();
    let args = RunArgs {
        agent_name: "codex".to_string(),
        repo: Some(repo.path().display().to_string()),
        dir: Some(wt.display().to_string()),
        dry_run: true,
        skills: vec![super::NO_SKILL_SENTINEL.to_string()],
        ..Default::default()
    };

    assert!(run_lifecycle(&store, &task_id, &args).await.is_none());

    assert_eq!(store.get_task(task_id.as_str()).unwrap().unwrap().status, TaskStatus::Failed);
    assert_eq!(std::fs::read_to_string(output_path).unwrap(), "partial output\n");
    assert_eq!(std::fs::read_to_string(wt.join("done.txt")).unwrap(), "done\n");
    git(repo.path(), &["rev-parse", "--verify", &format!("refs/heads/{branch}")]);
    assert!(wt.exists());
}
