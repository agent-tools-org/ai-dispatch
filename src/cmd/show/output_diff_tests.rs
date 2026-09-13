// Tests for start-SHA-aware diff rendering in `aid show`.
// Exports: none; validates task-scoped diff bases and failed-task no-op messaging.
// Deps: show_output hub, Store, Task, git CLI, tempfile.

use super::*;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

fn git(dir: &Path, args: &[&str]) {
    assert!(Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .unwrap()
        .success());
}

fn git_stdout(dir: &Path, args: &[&str]) -> String {
    String::from_utf8(
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string()
}

fn write_and_commit(dir: &Path, file: &str, content: &str, message: &str) {
    std::fs::write(dir.join(file), content).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "-m", message]);
}

fn init_repo(dir: &Path) {
    git(dir, &["init"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "user.name", "Test User"]);
    write_and_commit(dir, "base.txt", "base\n", "base");
    git(dir, &["branch", "-M", "main"]);
}

fn task_fixture(id: &str, repo: &Path, start_sha: Option<&str>, status: TaskStatus) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: Some(repo.display().to_string()), project_id: None,
        worktree_path: Some(repo.display().to_string()), effective_dir: None,
        worktree_branch: Some("task-branch".to_string()),
        final_head_sha: None,
        final_branch: None,
        start_sha: start_sha.map(|sha| sha.to_string()),
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
fn diff_text_uses_start_sha_to_exclude_prior_task_commits() {
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path());
    write_and_commit(repo.path(), "old.txt", "old task\n", "previous task");
    let start_sha = git_stdout(repo.path(), &["rev-parse", "HEAD"]);
    write_and_commit(repo.path(), "new.txt", "new task\n", "current task");

    let store = Arc::new(Store::open_memory().unwrap());
    let task = task_fixture("t-start-sha", repo.path(), Some(&start_sha), TaskStatus::Done);
    store.insert_task(&task).unwrap();

    let text = diff_text(&store, task.id.as_str()).unwrap();

    assert!(text.contains("new.txt"), "got: {text}");
    assert!(!text.contains("old.txt"), "got: {text}");
}

#[test]
fn failed_task_without_new_commits_reports_no_changes() {
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path());
    write_and_commit(repo.path(), "old.txt", "old task\n", "previous task");
    let start_sha = git_stdout(repo.path(), &["rev-parse", "HEAD"]);

    let store = Arc::new(Store::open_memory().unwrap());
    let task = task_fixture("t-no-commit", repo.path(), Some(&start_sha), TaskStatus::Failed);
    store.insert_task(&task).unwrap();

    let text = diff_text(&store, task.id.as_str()).unwrap();

    assert!(text.contains("No changes (task failed before making commits)"));
    assert!(!text.contains("old.txt"), "got: {text}");
}

#[test]
fn failed_task_with_untracked_only_worktree_reports_partial_work() {
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path());
    let start_sha = git_stdout(repo.path(), &["rev-parse", "HEAD"]);
    std::fs::write(repo.path().join("new.txt"), "partial\n").unwrap();

    let store = Arc::new(Store::open_memory().unwrap());
    let task = task_fixture("t-untracked", repo.path(), Some(&start_sha), TaskStatus::Failed);
    store.insert_task(&task).unwrap();

    let text = diff_text(&store, task.id.as_str()).unwrap();

    assert!(text.contains("Partial work present (uncommitted) at"), "got: {text}");
    assert!(text.contains("untracked: 1"), "got: {text}");
    assert!(text.contains("?? new.txt"), "got: {text}");
    assert!(!text.contains("(no changes detected)"), "got: {text}");
}

#[test]
fn branch_tip_equal_to_base_with_no_start_sha_shows_empty_diff() {
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path());
    write_and_commit(repo.path(), "release.txt", "release\n", "release");
    git(repo.path(), &["checkout", "-b", "task-branch"]);

    let store = Arc::new(Store::open_memory().unwrap());
    let task = task_fixture("t-clean-base", repo.path(), None, TaskStatus::Done);
    store.insert_task(&task).unwrap();

    let text = diff_text(&store, task.id.as_str()).unwrap();

    assert!(text.contains("(no changes detected)"), "got: {text}");
    assert!(!text.contains("release.txt"), "got: {text}");
}

#[test]
fn branch_with_one_commit_shows_only_task_commit_changes() {
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path());
    write_and_commit(repo.path(), "release.txt", "release\n", "release");
    git(repo.path(), &["checkout", "-b", "task-branch"]);
    write_and_commit(repo.path(), "task.txt", "task\n", "task");

    let store = Arc::new(Store::open_memory().unwrap());
    let task = task_fixture("t-one-commit", repo.path(), None, TaskStatus::Done);
    store.insert_task(&task).unwrap();

    let text = diff_text(&store, task.id.as_str()).unwrap();

    assert!(text.contains("task.txt"), "got: {text}");
    assert!(!text.contains("release.txt"), "got: {text}");
}

#[test]
fn branch_with_tracked_worktree_changes_still_shows_them() {
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path());
    write_and_commit(repo.path(), "release.txt", "release\n", "release");
    git(repo.path(), &["checkout", "-b", "task-branch"]);
    std::fs::write(repo.path().join("release.txt"), "release\nedited\n").unwrap();

    let store = Arc::new(Store::open_memory().unwrap());
    let task = task_fixture("t-dirty-tracked", repo.path(), None, TaskStatus::Done);
    store.insert_task(&task).unwrap();

    let text = diff_text(&store, task.id.as_str()).unwrap();

    assert!(text.contains("release.txt"), "got: {text}");
    assert!(text.contains("+edited"), "got: {text}");
}

#[test]
fn branch_diff_signal_shows_branch_totals_when_start_sha_is_ahead_of_base() {
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path());
    
    // Create an early commit on the task branch
    git(repo.path(), &["checkout", "-b", "task-branch"]);
    write_and_commit(repo.path(), "early.txt", "early\n", "early");
    
    // start_sha is after the early commit
    let start_sha = git_stdout(repo.path(), &["rev-parse", "HEAD"]);
    
    // Add another commit that the task just made
    write_and_commit(repo.path(), "task.txt", "task\n", "task");

    let store = Arc::new(Store::open_memory().unwrap());
    let task = task_fixture("t-branch-diff", repo.path(), Some(&start_sha), TaskStatus::Done);
    store.insert_task(&task).unwrap();

    let text = diff_text(&store, task.id.as_str()).unwrap();

    assert!(text.contains("This branch carries earlier commits that are not in the diff above"), "got: {text}");
    assert!(text.contains("Whole branch:"), "got: {text}");
    assert!(text.contains("2 files changed"), "stat should have total changed files: {text}");
    assert!(text.contains("--diff --branch"), "notice must name the command that shows it: {text}");
}

/// A clone that never created a local `main` still has `origin/main`. Resolving the base
/// to the bare name would make `git merge-base main HEAD` exit 128, and the branch view
/// would render empty while claiming to be the whole branch.
#[test]
fn branch_base_resolves_through_origin_when_no_local_default_branch_exists() {
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path());
    let main_sha = git_stdout(repo.path(), &["rev-parse", "HEAD"]);

    // Stand up origin/main + origin/HEAD, then drop the local branch entirely.
    git(repo.path(), &["update-ref", "refs/remotes/origin/main", &main_sha]);
    git(repo.path(), &["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"]);
    git(repo.path(), &["checkout", "-b", "task-branch"]);
    git(repo.path(), &["branch", "-D", "main"]);
    assert_eq!(
        branch_base_ref(repo.path().to_str().unwrap()).as_deref(),
        Some("origin/main"),
        "base must keep the remote prefix that actually resolves"
    );

    write_and_commit(repo.path(), "delivered.txt", "the actual work\n", "deliver");
    let start_sha = git_stdout(repo.path(), &["rev-parse", "HEAD"]);
    write_and_commit(repo.path(), "sliver.txt", "trailing tweak\n", "sliver");

    let store = Arc::new(Store::open_memory().unwrap());
    let task = task_fixture("t-origin-base", repo.path(), Some(&start_sha), TaskStatus::Done);
    store.insert_task(&task).unwrap();

    let text = diff_text(&store, task.id.as_str()).unwrap();
    assert!(text.contains("carries earlier commits"), "notice needs a resolvable base: {text}");

    let whole_branch = diff_text_branch(&store, task.id.as_str()).unwrap();
    assert!(!whole_branch.contains("Cannot locate this branch's base"), "base was resolvable: {whole_branch}");
    assert!(whole_branch.contains("delivered.txt"), "branch view must reach the earlier commit: {whole_branch}");
}

/// No default branch of any name resolves. The branch view must announce that it cannot
/// be built rather than labelling a fallback diff "whole branch".
#[test]
fn branch_mode_says_so_when_no_base_ref_resolves() {
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path());
    git(repo.path(), &["checkout", "-b", "task-branch"]);
    git(repo.path(), &["branch", "-D", "main"]);
    assert_eq!(branch_base_ref(repo.path().to_str().unwrap()), None, "no base should resolve here");

    let start_sha = git_stdout(repo.path(), &["rev-parse", "HEAD"]);
    write_and_commit(repo.path(), "sliver.txt", "trailing tweak\n", "sliver");

    let store = Arc::new(Store::open_memory().unwrap());
    let task = task_fixture("t-no-base", repo.path(), Some(&start_sha), TaskStatus::Done);
    store.insert_task(&task).unwrap();

    let whole_branch = diff_text_branch(&store, task.id.as_str()).unwrap();
    assert!(whole_branch.contains("Cannot locate this branch's base"), "must not fake a branch view: {whole_branch}");
    assert!(!whole_branch.contains("whole branch"), "no scope label it cannot honour: {whole_branch}");
    // The message promises this task's own changes; the diff has to actually contain them.
    assert!(whole_branch.contains("sliver.txt"), "the promised task-scoped fallback must be real: {whole_branch}");
    assert!(whole_branch.contains("trailing tweak"), "task-scoped fallback must carry content: {whole_branch}");
}

/// The sliver that misled a reviewer on t-325bd9df: the task's own diff is honest but
/// tiny, and the delivered file sits in an earlier commit on the same branch.
#[test]
fn branch_mode_shows_earlier_commits_that_task_diff_omits() {
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path());

    git(repo.path(), &["checkout", "-b", "task-branch"]);
    write_and_commit(repo.path(), "delivered.txt", "the actual work\n", "deliver");
    let start_sha = git_stdout(repo.path(), &["rev-parse", "HEAD"]);
    write_and_commit(repo.path(), "sliver.txt", "trailing tweak\n", "sliver");

    let store = Arc::new(Store::open_memory().unwrap());
    let task = task_fixture("t-branch-mode", repo.path(), Some(&start_sha), TaskStatus::Done);
    store.insert_task(&task).unwrap();

    let task_only = diff_text(&store, task.id.as_str()).unwrap();
    assert!(task_only.contains("sliver.txt"), "task diff must show its own commit: {task_only}");
    assert!(
        !task_only.contains("the actual work"),
        "task diff is scoped to start_sha..HEAD by design: {task_only}"
    );

    let whole_branch = diff_text_branch(&store, task.id.as_str()).unwrap();
    assert!(whole_branch.contains("delivered.txt"), "branch diff must reach the earlier commit: {whole_branch}");
    assert!(whole_branch.contains("the actual work"), "branch diff must include its content: {whole_branch}");
    assert!(whole_branch.contains("sliver.txt"), "branch diff must still include this task's own commit: {whole_branch}");
}
