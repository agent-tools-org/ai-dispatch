// Tests for final branch capture during `aid stop`.
// Covers stopped tasks preserving readable worktree branch state.
// Deps: stop command, Store, git CLI, worktree creation.

use super::{kill, stop};
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

fn running_task(id: &str, repo: &Path, wt: &Path, branch: &str) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Running,
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
fn stop_captures_final_branch_before_marking_stopped() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = "fix/stop-final-capture";
    let info = crate::worktree::create_worktree(repo.path(), branch, None).unwrap();
    git(&info.path, &["switch", "-c", "agent/stop-final"]);
    let store = Arc::new(Store::open_memory().unwrap());
    let task = running_task("t-stop-final-capture", repo.path(), &info.path, branch);
    store.insert_task(&task).unwrap();

    stop(&store, task.id.as_str()).unwrap();

    let loaded = store.get_task(task.id.as_str()).unwrap().unwrap();
    assert_eq!(loaded.status, TaskStatus::Stopped);
    assert_eq!(loaded.final_branch.as_deref(), Some("agent/stop-final"));
    assert!(loaded.final_head_sha.is_some());
    assert!(info.path.exists());
}

#[test]
fn stop_and_kill_release_worktree_locks() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let store = Arc::new(Store::open_memory().unwrap());

    for (task_id, branch, status, action) in [
        ("t-stop-lock", "fix/stop-lock", TaskStatus::Running, stop as fn(&Arc<Store>, &str) -> anyhow::Result<()>),
        ("t-kill-lock", "fix/kill-lock", TaskStatus::Running, kill as fn(&Arc<Store>, &str) -> anyhow::Result<()>),
        ("t-pending-lock", "fix/pending-lock", TaskStatus::Pending, stop as fn(&Arc<Store>, &str) -> anyhow::Result<()>),
    ] {
        let info = crate::worktree::create_worktree(repo.path(), branch, None).unwrap();
        let mut task = running_task(task_id, repo.path(), &info.path, branch);
        task.status = status;
        store.insert_task(&task).unwrap();
        crate::worktree::try_acquire_worktree_lock(&info.path, task_id).unwrap();

        action(&store, task_id).unwrap();

        assert!(!info.path.join(".aid-lock").exists());
        assert!(crate::worktree::check_worktree_lock(&info.path).is_none());
    }
}
