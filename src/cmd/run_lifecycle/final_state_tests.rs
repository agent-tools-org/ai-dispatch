// Tests for lifecycle final-state capture before failed worktree cleanup.
// Covers final branch persistence when fast-fail cleanup removes the worktree.
// Deps: run lifecycle, Store, git CLI, worktree creation.

use super::{
    run_lifecycle::{post_run_lifecycle, LifecycleMode},
    run_prompt::PromptBundle,
    RunArgs,
};
use crate::store::Store;
use crate::test_subprocess;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success());
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

fn failed_task(id: &str, repo: &Path, wt: &Path, branch: &str) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Failed,
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
        duration_ms: Some(100),
        model: None,
        cost_usd: None,
        exit_code: Some(1),
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

#[tokio::test]
async fn failed_lifecycle_captures_final_branch_before_fast_fail_cleanup() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = "fix/failure-final-capture";
    let info = crate::worktree::create_worktree(repo.path(), branch, None).unwrap();
    git(&info.path, &["switch", "-c", "agent/failure-final"]);
    std::fs::write(info.path.join("final.txt"), "final\n").unwrap();
    git(&info.path, &["add", "final.txt"]);
    git(&info.path, &["commit", "-m", "final work"]);
    let store = Arc::new(Store::open_memory().unwrap());
    let task = failed_task("t-failure-final-capture", repo.path(), &info.path, branch);
    store.insert_task(&task).unwrap();
    let prompt_bundle = PromptBundle {
        effective_prompt: "prompt".to_string(),
        context_files: Vec::new(),
        prompt_tokens: 0,
        injected_memory_ids: Vec::new(),
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
    assert!(!info.path.exists());
}
