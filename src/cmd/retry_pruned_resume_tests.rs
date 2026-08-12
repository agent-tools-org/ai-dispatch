// Regression tests for retrying tasks whose committed worktree was pruned.
// Exports: none.
// Deps: retry_task_to_run_args, Store, git CLI, tempfile.

use super::{RetryArgs, retry_task_to_run_args};
use crate::cmd::run::RunArgs;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::path::Path;
use std::process::Command;

fn git(repo: &Path, args: &[&str]) {
    assert!(Command::new("git")
        .args(["-C", &repo.to_string_lossy()])
        .args(args)
        .status()
        .unwrap()
        .success());
}

fn git_output(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-C", &repo.to_string_lossy()])
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

fn failed_task(repo: &Path, worktree: &Path, branch: &str) -> Task {
    Task {
        id: TaskId("t-pruned-saved-base".to_string()), agent: AgentKind::Codex,
        custom_agent_name: None, prompt: "retry prompt".to_string(), resolved_prompt: None,
        category: None, status: TaskStatus::Failed, parent_task_id: None, workgroup_id: None,
        caller_kind: None, caller_session_id: None, agent_session_id: None,
        repo_path: Some(repo.display().to_string()), worktree_path: Some(worktree.display().to_string()),
        worktree_branch: Some(branch.to_string()), final_head_sha: None, final_branch: None,
        start_sha: None, log_path: None, output_path: None, tokens: None, prompt_tokens: None,
        duration_ms: None, requested_model: None, observed_model: None, attribution_source: None,
        cost_usd: None, exit_code: None, created_at: Local::now(), completed_at: None,
        verify: None, verify_status: VerifyStatus::Skipped, pending_reason: None,
        read_only: false, budget: false, audit_verdict: None, audit_report_path: None,
        delivery_assessment: None,
    }
}

#[test]
fn retry_recreates_pruned_committed_worktree_at_branch_tip() {
    let _permit = crate::test_subprocess::acquire();
    let repo = init_repo();
    let branch = format!("fix/pruned-saved-base-{}", std::process::id());
    let first = crate::worktree::create_worktree(repo.path(), &branch, Some("main")).unwrap();
    std::fs::write(first.path.join("agent.txt"), "agent\n").unwrap();
    git(&first.path, &["add", "agent.txt"]);
    git(&first.path, &["commit", "-m", "agent commit"]);
    let branch_head = git_output(repo.path(), &["rev-parse", &branch]);
    let worktree_path = first.path.clone();
    git(repo.path(), &["worktree", "remove", "--force", &worktree_path.to_string_lossy()]);

    let task = failed_task(repo.path(), &worktree_path, &branch);
    let store = Store::open_memory().unwrap();
    store.insert_task(&task).unwrap();
    let saved = RunArgs {
        repo: Some(repo.path().display().to_string()),
        dir: Some(repo.path().display().to_string()),
        worktree: Some(branch.clone()), base_branch: Some("main".to_string()),
        agent_name: "codex".to_string(), prompt: task.prompt.clone(), ..Default::default()
    };
    store.update_task_dispatch_args(task.id.as_str(), &saved.dispatch_args_json().unwrap()).unwrap();

    let retry = retry_task_to_run_args(&store, &task, RetryArgs {
        task_id: task.id.to_string(), feedback: "continue".to_string(), agent: None,
        dir: None, reset: false, bg: false,
    }, false).unwrap();

    assert_eq!(retry.base_branch.as_deref(), Some(branch.as_str()));
    let recreated = crate::worktree::create_worktree(
        repo.path(), retry.worktree.as_deref().unwrap(), retry.base_branch.as_deref(),
    ).unwrap();
    assert_eq!(git_output(repo.path(), &["rev-parse", &branch]), branch_head);
    assert_eq!(git_output(&recreated.path, &["rev-parse", "HEAD"]), branch_head);
    assert_eq!(std::fs::read_to_string(recreated.path.join("agent.txt")).unwrap(), "agent\n");
}
