// Tests for task detail delivery reporting.
// Covers final commit stat, final branch display, and branch drift warnings.
// Deps: board::render_task_detail, git CLI, Task fixtures.

use super::render_task_detail;
use crate::test_subprocess;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success());
}

fn git_stdout(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn task(repo: &Path, start_sha: &str, final_sha: &str) -> Task {
    Task {
        id: TaskId("t-detail-final".to_string()),
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
        repo_path: Some(repo.to_string_lossy().to_string()), project_id: None,
        worktree_path: Some(repo.to_string_lossy().to_string()), effective_dir: None,
        worktree_branch: Some("dispatch/branch".to_string()),
        final_head_sha: Some(final_sha.to_string()),
        final_branch: Some("agent/final".to_string()),
        start_sha: Some(start_sha.to_string()),
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        requested_model: None, observed_model: None, attribution_source: None,
        cost_usd: None,
        exit_code: None,
        created_at: Local::now(),
        completed_at: None,
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

#[test]
fn detail_shows_delivered_commit_stat_and_branch_drift() {
    let _permit = test_subprocess::acquire();
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test User"]);
    std::fs::write(repo.path().join("base.txt"), "base\n").unwrap();
    git(repo.path(), &["add", "base.txt"]);
    git(repo.path(), &["commit", "-m", "base"]);
    let start_sha = git_stdout(repo.path(), &["rev-parse", "HEAD"]);
    git(repo.path(), &["switch", "-c", "agent/final"]);
    std::fs::write(repo.path().join("feature.txt"), "one\ntwo\n").unwrap();
    git(repo.path(), &["add", "feature.txt"]);
    git(repo.path(), &["commit", "-m", "final work"]);
    let final_sha = git_stdout(repo.path(), &["rev-parse", "HEAD"]);

    let output = render_task_detail(&task(repo.path(), &start_sha, &final_sha), &[], None);

    assert!(output.contains("Delivered: 1 files (+2/-0)"), "output: {output}");
    assert!(output.contains("\"final work\""), "output: {output}");
    assert!(output.contains("Branch:    agent/final"), "output: {output}");
    assert!(output.contains("agent switched branch: dispatch/branch -> agent/final"), "output: {output}");
}
