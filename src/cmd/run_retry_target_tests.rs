// Tests for retry target selection and legacy metadata failures.
// Covers live worktree retries when persisted repo metadata is incomplete.
// Deps: run retry-target helpers, git worktrees, task domain types.

use super::*;
use chrono::Local;
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

fn failed_task(id: &str) -> Task {
    Task {
        id: TaskId(id.to_string()), agent: AgentKind::Codex, custom_agent_name: None,
        prompt: "original prompt".to_string(), resolved_prompt: None, category: None,
        status: TaskStatus::Failed, parent_task_id: None, workgroup_id: None,
        caller_kind: None, caller_session_id: None, agent_session_id: None, repo_path: None,
        worktree_path: None, worktree_branch: None, final_head_sha: None, final_branch: None, start_sha: None,
        log_path: None, output_path: None, tokens: None, prompt_tokens: None, duration_ms: None,
        model: None, cost_usd: None, exit_code: None, created_at: Local::now(), completed_at: None,
        verify: None, verify_status: VerifyStatus::Skipped, pending_reason: None,
        read_only: false, budget: false, audit_verdict: None, audit_report_path: None,
        delivery_assessment: None,
    }
}

#[test]
fn live_worktree_retry_without_repo_path_fails_with_legacy_metadata_message() {
    let _permit = crate::test_subprocess::acquire();
    let repo = init_repo();
    let worktree_root = tempfile::tempdir().unwrap();
    let worktree = worktree_root.path().join("linked");
    git(
        repo.path(),
        &["worktree", "add", "-b", "aid/retry-legacy", &worktree.to_string_lossy()],
    );
    let mut task = failed_task("t-legacy-live-worktree");
    task.worktree_path = Some(worktree.to_string_lossy().to_string());
    task.worktree_branch = Some("aid/retry-legacy".to_string());
    let mut args = RunArgs {
        dir: Some(worktree.to_string_lossy().to_string()),
        ..Default::default()
    };

    let err = apply_retry_target(&task, &mut args).unwrap_err();

    assert!(err.to_string().contains("legacy task metadata"));
    assert!(err.to_string().contains("repo_path"));
}
