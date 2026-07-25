// Tests for dispatch guard fail-closed worktree task validation.
// Exports: none.
// Deps: run_dispatch_guard helpers, Task metadata, temp git repositories.
use super::*;
use chrono::Local;
use crate::types::{AgentKind, TaskId, TaskStatus, VerifyStatus};
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn task_with_branch(id: &str, branch: Option<&str>) -> Task {
    Task {
        id: TaskId(id.to_string()), agent: AgentKind::Codex, custom_agent_name: None,
        prompt: "prompt".to_string(), resolved_prompt: None, category: None,
        status: TaskStatus::Pending, parent_task_id: None, workgroup_id: None,
        caller_kind: None, caller_session_id: None, agent_session_id: None,
        repo_path: None, worktree_path: None, worktree_branch: branch.map(str::to_string),
        final_head_sha: None, final_branch: None, start_sha: None, log_path: None,
        output_path: None, tokens: None, prompt_tokens: None, duration_ms: None,
        model: None, cost_usd: None, exit_code: None, created_at: Local::now(),
        completed_at: None, verify: None, verify_status: VerifyStatus::Skipped,
        pending_reason: None, read_only: false, budget: false, audit_verdict: None,
        audit_report_path: None, delivery_assessment: None,
    }
}

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
    git(repo.path(), &["commit", "-m", "init"]);
    repo
}

fn unique_branch(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

#[test]
fn guard_rejects_worktree_task_resolved_to_repo_root() {
    let repo = tempfile::tempdir().unwrap();
    let task = task_with_branch("t-guard", Some("feat/guard"));
    let repo_path = repo.path().to_string_lossy().to_string();

    let err = ensure_worktree_task_not_repo_root(&task, Some(repo_path.as_str()), Some(repo_path.as_str()))
        .unwrap_err();

    let message = err.to_string();
    assert!(message.contains("t-guard"));
    assert!(message.contains("feat/guard"));
    assert!(message.contains(&repo_path));
}

#[test]
fn guard_allows_non_worktree_task_in_repo_root() {
    let repo = tempfile::tempdir().unwrap();
    let task = task_with_branch("t-main", None);
    let repo_path = repo.path().to_string_lossy().to_string();

    ensure_worktree_task_not_repo_root(&task, Some(repo_path.as_str()), Some(repo_path.as_str())).unwrap();
}

#[test]
fn guard_allows_worktree_task_in_distinct_dir() {
    let repo = tempfile::tempdir().unwrap();
    let worktree = tempfile::tempdir().unwrap();
    let task = task_with_branch("t-wt", Some("feat/wt"));
    let repo_path = repo.path().to_string_lossy().to_string();
    let worktree_path = worktree.path().to_string_lossy().to_string();

    ensure_worktree_task_not_repo_root(
        &task,
        Some(worktree_path.as_str()),
        Some(repo_path.as_str()),
    )
    .unwrap();
}

#[test]
fn guard_refuses_worktree_branch_without_derivable_repo_path() {
    let dir = tempfile::tempdir().unwrap();
    let task = task_with_branch("t-missing-repo", Some("feat/missing-repo"));
    let dir_path = dir.path().to_string_lossy().to_string();

    let err = ensure_worktree_task_not_repo_root(&task, Some(dir_path.as_str()), None)
        .unwrap_err();

    let message = err.to_string();
    assert!(message.contains("t-missing-repo"));
    assert!(message.contains("feat/missing-repo"));
    assert!(message.contains("could not resolve repository root"));
}

#[test]
fn guard_refuses_worktree_branch_without_resolved_dir_at_repo_root() {
    let task = task_with_branch("t-missing-dir", Some("feat/missing-dir"));
    let current_dir = std::env::current_dir().unwrap();
    let current_dir = current_dir.to_string_lossy().to_string();

    let err = ensure_worktree_task_not_repo_root(&task, None, Some(current_dir.as_str()))
        .unwrap_err();

    let message = err.to_string();
    assert!(message.contains("t-missing-dir"));
    assert!(message.contains("feat/missing-dir"));
    assert!(message.contains(&current_dir));
}

#[test]
fn guard_derives_repo_from_task_worktree_path_and_allows_real_worktree() {
    let _permit = crate::test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique_branch("feat/derive-worktree");
    let linked_root = tempfile::tempdir().unwrap();
    let linked_path = linked_root.path().join("linked");
    git(
        repo.path(),
        &[
            "worktree",
            "add",
            "-b",
            branch.as_str(),
            &linked_path.to_string_lossy(),
        ],
    );
    let worktree_path = linked_path.to_string_lossy().to_string();
    let mut task = task_with_branch("t-derived", Some(branch.as_str()));
    task.worktree_path = Some(worktree_path.clone());

    ensure_worktree_task_not_repo_root(&task, Some(worktree_path.as_str()), None).unwrap();

    git(repo.path(), &["worktree", "remove", "--force", &worktree_path]);
}

#[test]
fn t_b162bd4a_incident_shape_refuses_repo_root_without_repo_path() {
    let repo = init_repo();
    let repo_path = repo.path().to_string_lossy().to_string();
    let task = task_with_branch("t-b162bd4a", Some("feat/aid-build"));

    let err = ensure_worktree_task_not_repo_root(&task, Some(repo_path.as_str()), None)
        .unwrap_err();

    let message = err.to_string();
    assert!(message.contains("t-b162bd4a"));
    assert!(message.contains("feat/aid-build"));
    assert!(message.contains(&repo_path));
}
