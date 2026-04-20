// Tests for successful-task worktree pruning after completion.
// Covers default auto-prune, config opt-out, shared-worktree preservation, and no-commit skips.
// Deps: super::cleanup_completed_worktree, Store, git CLI, tempfile.

use super::{cleanup_completed_worktree, create_worktree};
use crate::store::Store;
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
    std::fs::write(repo.path().join("base.txt"), "base\n").unwrap();
    git(repo.path(), &["add", "base.txt"]);
    git(repo.path(), &["commit", "-m", "base"]);
    repo
}

fn stored_task(id: &str, repo: &Path, wt: &Path, branch: &str, status: TaskStatus) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status,
        parent_task_id: None,
        workgroup_id: Some("wg-test".to_string()),
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: Some(repo.display().to_string()),
        worktree_path: Some(wt.display().to_string()),
        worktree_branch: Some(branch.to_string()),
        start_sha: None,
        log_path: None,
        output_path: None,
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

#[test]
fn cleanup_completed_worktree_removes_done_worktree_with_commits() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = "fix/cleanup-done";
    let info = create_worktree(repo.path(), branch, None).unwrap();
    std::fs::write(info.path.join("done.txt"), "done\n").unwrap();
    git(&info.path, &["add", "done.txt"]);
    git(&info.path, &["commit", "-m", "done"]);
    let store = Store::open_memory().unwrap();
    store
        .insert_task(&stored_task("t-done", repo.path(), &info.path, branch, TaskStatus::Done))
        .unwrap();

    cleanup_completed_worktree(&store, &TaskId("t-done".to_string())).unwrap();

    assert!(!info.path.exists());
}

#[test]
fn cleanup_completed_worktree_honors_keep_config() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    std::fs::create_dir_all(repo.path().join(".aid")).unwrap();
    std::fs::write(
        repo.path().join(".aid/project.toml"),
        "[project]\nid = \"demo\"\nkeep_worktrees_after_done = true\n",
    )
    .unwrap();
    let branch = "fix/keep-done";
    let info = create_worktree(repo.path(), branch, None).unwrap();
    std::fs::write(info.path.join("done.txt"), "done\n").unwrap();
    git(&info.path, &["add", "done.txt"]);
    git(&info.path, &["commit", "-m", "done"]);
    let store = Store::open_memory().unwrap();
    store
        .insert_task(&stored_task("t-keep", repo.path(), &info.path, branch, TaskStatus::Done))
        .unwrap();

    cleanup_completed_worktree(&store, &TaskId("t-keep".to_string())).unwrap();

    assert!(info.path.exists());
    git(
        repo.path(),
        &["worktree", "remove", "--force", &info.path.to_string_lossy()],
    );
}

#[test]
fn cleanup_completed_worktree_preserves_shared_active_worktree() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = "fix/shared-done";
    let info = create_worktree(repo.path(), branch, None).unwrap();
    std::fs::write(info.path.join("done.txt"), "done\n").unwrap();
    git(&info.path, &["add", "done.txt"]);
    git(&info.path, &["commit", "-m", "done"]);
    let store = Store::open_memory().unwrap();
    store
        .insert_task(&stored_task("t-first", repo.path(), &info.path, branch, TaskStatus::Done))
        .unwrap();
    store
        .insert_task(&stored_task(
            "t-second",
            repo.path(),
            &info.path,
            branch,
            TaskStatus::Running,
        ))
        .unwrap();

    cleanup_completed_worktree(&store, &TaskId("t-first".to_string())).unwrap();

    assert!(info.path.exists());
    git(
        repo.path(),
        &["worktree", "remove", "--force", &info.path.to_string_lossy()],
    );
}

#[test]
fn cleanup_completed_worktree_skips_branches_without_commits() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = "fix/no-commit";
    let info = create_worktree(repo.path(), branch, None).unwrap();
    let store = Store::open_memory().unwrap();
    store
        .insert_task(&stored_task("t-empty", repo.path(), &info.path, branch, TaskStatus::Done))
        .unwrap();

    cleanup_completed_worktree(&store, &TaskId("t-empty".to_string())).unwrap();

    assert!(info.path.exists());
    git(
        repo.path(),
        &["worktree", "remove", "--force", &info.path.to_string_lossy()],
    );
}
