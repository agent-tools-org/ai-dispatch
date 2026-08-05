// Tests for cascade retry argument inheritance.
// Covers worktree/dir reuse when a fallback agent takes over a failed task.
// Deps: run_lifecycle helper, RunArgs, task domain types.

use super::{run_lifecycle::inherit_cascade_target, RunArgs};
use crate::test_subprocess;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::path::Path;
use std::process::Command;

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
    std::fs::write(repo.path().join("file.txt"), "hello\n").unwrap();
    git(repo.path(), &["add", "file.txt"]);
    git(repo.path(), &["commit", "-m", "init"]);
    repo
}

fn failed_task(path: &str) -> Task {
    Task {
        id: TaskId("t-cascade".to_string()), agent: AgentKind::Codex, custom_agent_name: None,
        prompt: "prompt".to_string(), resolved_prompt: None, category: None,
        status: TaskStatus::Failed, parent_task_id: None, workgroup_id: None,
        caller_kind: None, caller_session_id: None, agent_session_id: None, repo_path: None,
        worktree_path: Some(path.to_string()), worktree_branch: Some("feat/cascade".to_string()),
        final_head_sha: None,
        final_branch: None,
        start_sha: None, log_path: None, output_path: None, tokens: None, prompt_tokens: None,
        duration_ms: None, requested_model: None, observed_model: None, attribution_source: None, cost_usd: None, exit_code: None,
        created_at: Local::now(), completed_at: None, verify: None,
        verify_status: VerifyStatus::Skipped, pending_reason: None, read_only: false,
        budget: false, audit_verdict: None, audit_report_path: None, delivery_assessment: None,
    }
}

fn failed_task_with_repo(repo: &str, path: &str) -> Task {
    let mut task = failed_task(path);
    task.repo_path = Some(repo.to_string());
    task
}

#[test]
fn cascade_inherits_existing_worktree_dir() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let linked_root = tempfile::tempdir().unwrap();
    let linked = linked_root.path().join("linked");
    git(repo.path(), &["worktree", "add", "-b", "feat/cascade", &linked.to_string_lossy()]);
    let task = failed_task(&linked.display().to_string());
    let mut args = RunArgs {
        agent_name: "gemini".to_string(),
        worktree: Some("feat/cascade".to_string()),
        ..Default::default()
    };

    inherit_cascade_target(&mut args, &task).unwrap();

    assert_eq!(args.dir, task.worktree_path);
    // The cascade keeps the worktree branch so the follow-up task is persisted with its
    // worktree metadata intact; dropping it strips isolation from later generations.
    assert_eq!(args.worktree.as_deref(), Some("feat/cascade"));
    git(repo.path(), &["worktree", "remove", "--force", &linked.to_string_lossy()]);
}

#[test]
fn cascade_refuses_persisted_worktree_that_equals_repo_path() {
    let temp = tempfile::tempdir().unwrap();
    let repo_path = temp.path().display().to_string();
    let task = failed_task_with_repo(&repo_path, &repo_path);
    let mut args = RunArgs {
        agent_name: "gemini".to_string(),
        worktree: Some("feat/cascade".to_string()),
        ..Default::default()
    };

    let err = inherit_cascade_target(&mut args, &task).unwrap_err();

    assert!(err.to_string().contains("recorded worktree path"));
}
