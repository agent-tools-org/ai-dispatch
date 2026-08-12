// Tests for failed-task auto-retry worktree target recovery.
// Covers pruned worktree directories, branch-tip recreation, and fail-closed errors.
// Deps: retry_logic builder, Store, git CLI, worktree helpers.

use super::*;
use chrono::Local;
use crate::types::{AgentKind, VerifyStatus};
use std::path::Path;
use std::process::Command;

fn git(repo_dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy()])
        .args(args)
        .status()
        .unwrap();
    assert!(status.success());
}

fn git_output(repo_dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy()])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn init_repo() -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test User"]);
    std::fs::write(repo.path().join("base.txt"), "base\n").unwrap();
    git(repo.path(), &["add", "base.txt"]);
    git(repo.path(), &["commit", "-m", "init"]);
    repo
}

fn failed_task(id: &str) -> Task {
    Task {
        id: TaskId(id.to_string()), agent: AgentKind::Codex, prompt: "prompt".to_string(),
        custom_agent_name: None,
        resolved_prompt: None, category: None, status: TaskStatus::Failed, parent_task_id: None, workgroup_id: None,
        caller_kind: None, caller_session_id: None, agent_session_id: None, repo_path: None, project_id: None, worktree_path: None, effective_dir: None,
        worktree_branch: None, final_head_sha: None, final_branch: None, start_sha: None, log_path: None, output_path: None, tokens: None, prompt_tokens: None,
        duration_ms: None, requested_model: None, observed_model: None, attribution_source: None, cost_usd: None, exit_code: None, created_at: Local::now(),
        completed_at: None, verify: None, verify_status: VerifyStatus::Skipped, pending_reason: None, read_only: false, budget: false,
        audit_verdict: None, audit_report_path: None, delivery_assessment: None,
    }
}

fn retry_args_for(store: &Store, task: &Task) -> Result<RunArgs> {
    build_failed_retry_args(
        store,
        &task.id,
        &RunArgs { retry: 1, prompt: "fallback prompt".to_string(), ..Default::default() },
        task,
        "agent failed",
    )
}

#[test]
fn failed_retry_with_pruned_worktree_keeps_branch_target() {
    let _permit = crate::test_subprocess::acquire();
    let store = Store::open_memory().unwrap();
    let repo = init_repo();
    let branch = format!("fix/retry-pruned-{}", std::process::id());
    git(repo.path(), &["branch", &branch]);
    let missing = repo.path().join("missing-worktree");
    let mut task = failed_task("t-pruned-retry");
    task.repo_path = Some(repo.path().display().to_string());
    task.worktree_path = Some(missing.display().to_string());
    task.worktree_branch = Some(branch.clone());
    store.insert_task(&task).unwrap();

    let retry_args = retry_args_for(&store, &task).unwrap();

    assert_eq!(retry_args.worktree.as_deref(), Some(branch.as_str()));
    assert_eq!(retry_args.dir.as_deref(), Some(repo.path().to_str().unwrap()));
    assert_eq!(retry_args.base_branch.as_deref(), Some(branch.as_str()));
}

#[test]
fn pruned_retry_recreates_worktree_from_parent_branch_tip() {
    let _permit = crate::test_subprocess::acquire();
    let store = Store::open_memory().unwrap();
    let repo = init_repo();
    let branch = format!("fix/retry-preserve-{}", std::process::id());
    let worktree = crate::worktree::create_worktree(repo.path(), &branch, Some("main")).unwrap();
    std::fs::write(worktree.path.join("sentinel.txt"), "keep\n").unwrap();
    git(&worktree.path, &["add", "sentinel.txt"]);
    git(&worktree.path, &["commit", "-m", "parent commit"]);
    let parent_head = git_output(repo.path(), &["rev-parse", &branch]);
    git(repo.path(), &["worktree", "remove", "--force", &worktree.path.to_string_lossy()]);
    let mut task = failed_task("t-recreate-retry");
    task.repo_path = Some(repo.path().display().to_string());
    task.worktree_path = Some(worktree.path.display().to_string());
    task.worktree_branch = Some(branch.clone());
    store.insert_task(&task).unwrap();

    let retry_args = retry_args_for(&store, &task).unwrap();
    let recreated = crate::worktree::create_worktree(
        repo.path(),
        retry_args.worktree.as_deref().unwrap(),
        retry_args.base_branch.as_deref(),
    )
    .unwrap();

    assert_eq!(git_output(repo.path(), &["rev-parse", &branch]), parent_head);
    assert_eq!(std::fs::read_to_string(recreated.path.join("sentinel.txt")).unwrap(), "keep\n");
}

#[test]
fn failed_retry_errors_when_recorded_branch_is_gone() {
    let _permit = crate::test_subprocess::acquire();
    let store = Store::open_memory().unwrap();
    let repo = init_repo();
    let branch = format!("fix/retry-missing-{}", std::process::id());
    let mut task = failed_task("t-unrecoverable-retry");
    task.repo_path = Some(repo.path().display().to_string());
    task.worktree_path = Some(repo.path().join("missing-worktree").display().to_string());
    task.worktree_branch = Some(branch.clone());
    store.insert_task(&task).unwrap();

    let message = match retry_args_for(&store, &task) {
        Ok(_) => panic!("retry unexpectedly recovered missing branch"),
        Err(err) => err.to_string(),
    };

    assert!(message.contains("t-unrecoverable-retry"));
    assert!(message.contains(&branch));
    assert!(message.contains("branch does not exist"));
    assert!(message.contains("refusing to run in repo root"));
}
