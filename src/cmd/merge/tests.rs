// Tests for aid merge — unit tests for git helpers + integration tests for merge workflows.
// Deps: super::*, test_subprocess, tempfile

use super::*;
use crate::support::test_subprocess;
use crate::types::*;
use chrono::Local;
use std::env;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::TempDir;

fn git(repo: &Path, args: &[&str]) {
    let s = Command::new("git")
        .args(["-C", &repo.to_string_lossy()])
        .args(args)
        .output()
        .unwrap();
    assert!(
        s.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&s.stderr)
    );
}

fn unique(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

/// Create a git repo with one commit. Returns the TempDir.
fn init_repo() -> TempDir {
    let repo = TempDir::new().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@aid.dev"]);
    git(repo.path(), &["config", "user.name", "Test"]);
    std::fs::write(repo.path().join("init.txt"), "init\n").unwrap();
    git(repo.path(), &["add", "init.txt"]);
    git(repo.path(), &["commit", "-m", "init"]);
    repo
}

/// Create a worktree branch with one committed change. Returns (worktree_dir, branch_name).
fn create_worktree_with_commit(repo: &Path) -> (TempDir, String) {
    let branch = unique("test-branch");
    let wt = TempDir::new().unwrap();
    git(
        repo,
        &[
            "worktree",
            "add",
            &wt.path().to_string_lossy(),
            "-b",
            &branch,
        ],
    );
    std::fs::write(wt.path().join("agent-work.txt"), "agent output\n").unwrap();
    git(wt.path(), &["add", "agent-work.txt"]);
    git(wt.path(), &["commit", "-m", "agent: implement feature"]);
    (wt, branch)
}

fn create_empty_worktree_branch(repo: &Path) -> (TempDir, String) {
    let branch = unique("empty-branch");
    let wt = TempDir::new().unwrap();
    git(
        repo,
        &[
            "worktree",
            "add",
            &wt.path().to_string_lossy(),
            "-b",
            &branch,
        ],
    );
    (wt, branch)
}

fn create_conflict_worktree(repo: &Path, branch: &str) -> TempDir {
    let wt = TempDir::new().unwrap();
    git(
        repo,
        &[
            "worktree",
            "add",
            &wt.path().to_string_lossy(),
            "-b",
            branch,
        ],
    );
    std::fs::write(wt.path().join("init.txt"), "branch version\n").unwrap();
    git(wt.path(), &["add", "init.txt"]);
    git(wt.path(), &["commit", "-m", "branch change"]);
    std::fs::write(repo.join("init.txt"), "main version\n").unwrap();
    git(repo, &["add", "init.txt"]);
    git(repo, &["commit", "-m", "main change"]);
    wt
}

fn worktree_status(repo: &Path) -> String {
    let output = Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "status", "--short"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn head_message(repo: &Path) -> String {
    let output = Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "log", "-1", "--pretty=%s"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn make_task_with_worktree(id: &str, repo: &Path, wt: &Path, branch: &str) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "test".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: Some(repo.to_string_lossy().to_string()),
        project_id: None,
        worktree_path: Some(wt.to_string_lossy().to_string()),
        effective_dir: None,
        worktree_branch: Some(branch.to_string()),
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        requested_model: None,
        observed_model: None,
        attribution_source: None,
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

#[path = "force_tests.rs"]
mod force_tests;
#[path = "git_tests.rs"]
mod git_tests;
#[path = "group_tests.rs"]
mod group_tests;
#[path = "lock_tests.rs"]
mod lock_tests;
#[path = "merge_check_tests.rs"]
mod merge_check_tests;
#[path = "single_tests.rs"]
mod single_tests;
#[path = "stash_tests.rs"]
mod stash_tests;
