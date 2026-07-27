// Tests for lifecycle ordering around completed worktree cleanup.
// Covers retry dispatch preserving worktrees and normal finish pruning them.
// Deps: post_run_lifecycle, Store, worktree helpers, git CLI, tempfile.

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
use std::{path::Path, process::Command, sync::Arc};

fn git(repo_dir: &Path, args: &[&str]) {
    assert!(Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy()])
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

fn task(id: &str, repo: &Path, wt: &Path, branch: &str, output_path: &Path) -> Task {
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
        output_path: Some(output_path.display().to_string()),
        tokens: None,
        prompt_tokens: None,
        duration_ms: Some(1_000),
        model: None,
        cost_usd: None,
        exit_code: None,
        created_at: Local::now(),
        completed_at: Some(Local::now()),
        verify: None,
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

fn committed_worktree(repo: &Path, branch: &str) -> std::path::PathBuf {
    let info = crate::worktree::create_worktree(repo, branch, None).unwrap();
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
        TaskStatus::Done,
        None,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn completed_worktree_survives_when_checklist_retry_dispatches() {
    let _permit = test_subprocess::acquire();
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

    let retry_id = run_lifecycle(&store, &task_id, &args).await;

    assert!(retry_id.is_some());
    assert!(wt.exists());
}

#[tokio::test]
async fn completed_worktree_is_pruned_when_no_retry_dispatches() {
    let _permit = test_subprocess::acquire();
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
