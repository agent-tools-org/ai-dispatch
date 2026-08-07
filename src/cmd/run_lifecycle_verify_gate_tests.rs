// Lifecycle verify gate regressions for completed worktree tasks.
// Covers normal worktree completion and dirty-rescue completion paths.
// Deps: post_run_lifecycle, Store, git CLI, tempfile.

use super::{
    RunArgs,
    run_lifecycle::{LifecycleMode, post_run_lifecycle},
    run_prompt::PromptBundle,
};
use crate::{
    store::Store,
    test_subprocess,
    types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus},
};
use chrono::Local;
use std::{path::{Path, PathBuf}, process::Command, sync::Arc};

fn git(dir: &Path, args: &[&str]) {
    assert!(Command::new("git")
        .args(["-C", &dir.to_string_lossy()])
        .args(args)
        .status()
        .unwrap()
        .success());
}

fn init_repo() -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test User"]);
    std::fs::write(repo.path().join("base.txt"), "base\n").unwrap();
    git(repo.path(), &["add", "base.txt"]);
    git(repo.path(), &["commit", "-m", "base"]);
    repo
}

fn create_worktree(repo: &Path, branch: &str) -> PathBuf {
    let info = crate::worktree::create_worktree(repo, branch, None).unwrap();
    git(&info.path, &["config", "user.email", "test@example.com"]);
    git(&info.path, &["config", "user.name", "Test User"]);
    info.path
}

fn task(id: &str, repo: &Path, wt: &Path, branch: &str) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: Some(repo.display().to_string()),
        worktree_path: Some(wt.display().to_string()),
        worktree_branch: Some(branch.to_string()),
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: Some(1_000),
        requested_model: None, observed_model: None, attribution_source: None,
        cost_usd: None,
        exit_code: Some(0),
        created_at: Local::now(),
        completed_at: Some(Local::now()),
        verify: Some("false".to_string()),
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
    }
}

fn prompt_bundle() -> PromptBundle {
    PromptBundle {
        effective_prompt: "prompt".to_string(),
        context_files: Vec::new(),
        prompt_tokens: 0,
        injected_memory_ids: Vec::new(),
    }
}

async fn run_lifecycle(store: &Arc<Store>, task_id: &TaskId, args: &RunArgs) {
    post_run_lifecycle(
        LifecycleMode::Background,
        store,
        task_id,
        args,
        AgentKind::Codex,
        "codex",
        args.dir.as_ref(),
        args.repo.as_ref(),
        args.dir.as_ref(),
        None,
        &[],
        &prompt_bundle(),
        TaskStatus::Done,
        None,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn failed_verify_fails_completed_worktree_task() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = "fix/verify-gate-clean";
    let wt = create_worktree(repo.path(), branch);
    // Leave a real change so empty-diff skip does not fire; verify must still run.
    std::fs::write(wt.join("change.txt"), "changed\n").unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-vgate-worktree".to_string());
    store
        .insert_task(&task(task_id.as_str(), repo.path(), &wt, branch))
        .unwrap();
    let args = RunArgs {
        repo: Some(repo.path().display().to_string()),
        dir: Some(wt.display().to_string()),
        verify: Some("false".to_string()),
        ..Default::default()
    };

    run_lifecycle(&store, &task_id, &args).await;

    let task = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(task.verify_status, VerifyStatus::Failed);
    assert_eq!(task.status, TaskStatus::Failed);
    assert_eq!(task.exit_code, Some(1));
}

#[tokio::test]
async fn failed_verify_fails_after_dirty_rescue_path() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = "fix/verify-gate-rescue";
    let wt = create_worktree(repo.path(), branch);
    std::fs::write(wt.join("rescued.txt"), "rescued\n").unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-vgate-rescue".to_string());
    store
        .insert_task(&task(task_id.as_str(), repo.path(), &wt, branch))
        .unwrap();
    let args = RunArgs {
        repo: Some(repo.path().display().to_string()),
        dir: Some(wt.display().to_string()),
        verify: Some("false".to_string()),
        ..Default::default()
    };

    run_lifecycle(&store, &task_id, &args).await;

    let task = store.get_task(task_id.as_str()).unwrap().unwrap();
    let events = store.get_events(task_id.as_str()).unwrap();
    assert_eq!(task.verify_status, VerifyStatus::Failed);
    assert_eq!(task.status, TaskStatus::Failed);
    assert_eq!(task.exit_code, Some(1));
    assert!(events.iter().any(|event| event.detail.contains("Rescued 1 file")));
}
