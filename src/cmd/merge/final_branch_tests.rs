// Tests for merge source branch resolution after agents switch branches.
// Covers final_branch preference, drift confirmation, and real-branch merging.
// Deps: parent merge module helpers, Store, git CLI.

use super::*;
use crate::store::Store;
use crate::test_subprocess;
use crate::types::{AgentKind, VerifyStatus};
use chrono::Local;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_repo() -> TempDir {
    let repo = TempDir::new().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@aid.dev"]);
    git(repo.path(), &["config", "user.name", "Test"]);
    std::fs::write(repo.path().join("base.txt"), "base\n").unwrap();
    git(repo.path(), &["add", "base.txt"]);
    git(repo.path(), &["commit", "-m", "base"]);
    repo
}

fn task(repo: &Path, dispatch_branch: &str, final_branch: &str) -> Task {
    Task {
        id: TaskId("t-final-merge".to_string()),
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
        repo_path: Some(repo.to_string_lossy().to_string()), project_id: None,
        worktree_path: None,
        worktree_branch: Some(dispatch_branch.to_string()),
        final_head_sha: None,
        final_branch: Some(final_branch.to_string()),
        start_sha: None,
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
fn drifted_merge_requires_force_then_merges_final_branch() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    git(repo.path(), &["branch", "dispatch/branch"]);
    git(repo.path(), &["switch", "-c", "agent/final"]);
    std::fs::write(repo.path().join("final.txt"), "final\n").unwrap();
    git(repo.path(), &["add", "final.txt"]);
    git(repo.path(), &["commit", "-m", "final work"]);
    git(repo.path(), &["switch", "main"]);
    let store = Store::open_memory().unwrap();
    let task = task(repo.path(), "dispatch/branch", "agent/final");
    store.insert_task(&task).unwrap();

    let blocked = merge_single(&store, task.id.as_str(), false, false, false, None);
    assert!(blocked.is_err());
    assert!(!repo.path().join("final.txt").exists());

    merge_single(&store, task.id.as_str(), false, false, true, None).unwrap();

    assert!(repo.path().join("final.txt").exists());
    let loaded = store.get_task(task.id.as_str()).unwrap().unwrap();
    assert_eq!(loaded.status, TaskStatus::Merged);
}
