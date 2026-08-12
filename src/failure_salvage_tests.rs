// Tests for failed-task partial work salvage.
// Covers dirty WIP commit creation, clean no-op behavior, and best-effort failure.
// Deps: failure_salvage module, Store, AidHomeGuard, git CLI.

use crate::paths::{self, AidHomeGuard};
use crate::store::{Store, TaskCompletionUpdate};
use crate::types::{AgentKind, EventKind, Task, TaskEvent, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git command");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_stdout(dir: &Path, args: &[&str]) -> String {
    String::from_utf8(
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .expect("git command")
            .stdout,
    )
    .expect("utf8")
    .trim()
    .to_string()
}

fn init_repo() -> tempfile::TempDir {
    let repo = tempfile::tempdir().expect("tempdir");
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test User"]);
    std::fs::write(repo.path().join("base.txt"), "base\n").expect("write base");
    git(repo.path(), &["add", "base.txt"]);
    git(repo.path(), &["commit", "-m", "base"]);
    repo
}

fn task(id: &str, worktree_path: Option<String>) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Failed,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: worktree_path.clone(), project_id: None,
        worktree_path,
        worktree_branch: Some("feat/salvage".to_string()),
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

fn insert_activity(store: &Store, task_id: &TaskId) {
    for (kind, detail) in [
        (EventKind::Milestone, "started editing"),
        (EventKind::ToolCall, "write src/lib.rs"),
    ] {
        store
            .insert_event(&TaskEvent {
                task_id: task_id.clone(),
                timestamp: Local::now(),
                event_kind: kind,
                detail: detail.to_string(),
                metadata: None,
            })
            .expect("event");
    }
}

fn isolated_home() -> (tempfile::TempDir, AidHomeGuard) {
    let home = tempfile::tempdir().expect("home");
    let guard = AidHomeGuard::set(home.path());
    paths::ensure_dirs().expect("dirs");
    (home, guard)
}

#[test]
fn salvage_writes_partial_work_and_commits_dirty_worktree() {
    let (_home, _guard) = isolated_home();
    let repo = init_repo();
    std::fs::write(repo.path().join("base.txt"), "changed\n").expect("write");
    std::fs::write(repo.path().join("new.txt"), "new\n").expect("write");
    let store = Store::open_memory().expect("store");
    let mut task = task("t-salvage-dirty", Some(repo.path().display().to_string()));
    task.status = TaskStatus::Running;
    store.insert_task(&task).expect("insert");
    insert_activity(&store, &task.id);

    crate::task_lifecycle::update_task_completion(&store, TaskCompletionUpdate {
            id: task.id.as_str(),
            status: TaskStatus::Failed,
            tokens: None,
            duration_ms: 10,
            observed_model: None, attribution_source: None,
            cost_usd: None,
            exit_code: Some(1),
        })
        .expect("fail");

    let partial = std::fs::read_to_string(paths::task_dir(task.id.as_str()).join("partial-work.md"))
        .expect("partial");
    assert!(partial.contains("modified: 1, staged: 0, untracked: 1"));
    assert!(partial.contains("?? new.txt"));
    assert!(partial.contains("write src/lib.rs"));
    assert_eq!(
        git_stdout(repo.path(), &["log", "-1", "--pretty=%s"]),
        "wip: partial work salvage (task t-salvage-dirty failed)"
    );
    assert_eq!(git_stdout(repo.path(), &["status", "--porcelain"]), "");
}

#[test]
fn salvage_records_file_write_only_activity() {
    let (_home, _guard) = isolated_home();
    let repo = init_repo();
    std::fs::write(repo.path().join("base.txt"), "edited\n").expect("write");
    let store = Store::open_memory().expect("store");
    let mut task = task("t-salvage-file-write", Some(repo.path().display().to_string()));
    task.status = TaskStatus::Running;
    store.insert_task(&task).expect("insert");
    store
        .insert_event(&TaskEvent {
            task_id: task.id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::FileWrite,
            detail: "Edit files: src/lib.rs".to_string(),
            metadata: None,
        })
        .expect("event");

    crate::task_lifecycle::mark_failed(&store, &task.id).expect("fail");

    let partial = std::fs::read_to_string(paths::task_dir(task.id.as_str()).join("partial-work.md"))
        .expect("partial");
    assert!(partial.contains("file_write: Edit files: src/lib.rs"));
    assert!(!partial.contains("no milestone/tool_call/file_read/file_write events recorded"));
}

#[test]
fn salvage_records_file_read_only_activity() {
    let (_home, _guard) = isolated_home();
    let repo = init_repo();
    std::fs::write(repo.path().join("base.txt"), "edited\n").expect("write");
    let store = Store::open_memory().expect("store");
    let mut task = task("t-salvage-file-read", Some(repo.path().display().to_string()));
    task.status = TaskStatus::Running;
    store.insert_task(&task).expect("insert");
    store
        .insert_event(&TaskEvent {
            task_id: task.id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::FileRead,
            detail: "Read file: src/lib.rs".to_string(),
            metadata: None,
        })
        .expect("event");

    crate::task_lifecycle::mark_failed(&store, &task.id).expect("fail");

    let partial = std::fs::read_to_string(paths::task_dir(task.id.as_str()).join("partial-work.md"))
        .expect("partial");
    assert!(partial.contains("file_read: Read file: src/lib.rs"));
    assert!(!partial.contains("no milestone/tool_call/file_read/file_write events recorded"));
}

#[test]
fn salvage_noops_when_worktree_is_clean() {
    let (_home, _guard) = isolated_home();
    let repo = init_repo();
    let before = git_stdout(repo.path(), &["rev-parse", "HEAD"]);
    let store = Store::open_memory().expect("store");
    let mut task = task("t-salvage-clean", Some(repo.path().display().to_string()));
    task.status = TaskStatus::Running;
    store.insert_task(&task).expect("insert");

    crate::task_lifecycle::mark_failed(&store, &task.id).expect("fail");

    assert_eq!(git_stdout(repo.path(), &["rev-parse", "HEAD"]), before);
    assert!(!paths::task_dir(task.id.as_str()).join("partial-work.md").exists());
}

#[test]
fn salvage_excludes_aid_runtime_files_from_commit() {
    let (_home, _guard) = isolated_home();
    let repo = init_repo();
    std::fs::write(repo.path().join("base.txt"), "changed\n").expect("write");
    std::fs::write(repo.path().join(".aid-lock"), "pid=1234\n").expect("write lock");
    std::fs::write(repo.path().join(".aid-verify-deps-state"), "fresh=1\n").expect("write state");
    let store = Store::open_memory().expect("store");
    let mut task = task("t-salvage-aid-files", Some(repo.path().display().to_string()));
    task.status = TaskStatus::Running;
    store.insert_task(&task).expect("insert");

    crate::task_lifecycle::mark_failed(&store, &task.id).expect("fail");

    let committed = git_stdout(
        repo.path(),
        &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"],
    );
    assert!(committed.lines().any(|line| line == "base.txt"), "got: {committed}");
    assert!(!committed.lines().any(|line| line == ".aid-lock"), "got: {committed}");
    assert!(
        !committed.lines().any(|line| line == ".aid-verify-deps-state"),
        "got: {committed}"
    );
    // The files stay on disk (untouched), just never staged/committed by aid.
    assert!(repo.path().join(".aid-lock").exists());
    assert!(repo.path().join(".aid-verify-deps-state").exists());
}

#[test]
fn salvage_excludes_aid_directory_from_commit() {
    let (_home, _guard) = isolated_home();
    let repo = init_repo();
    std::fs::write(repo.path().join("base.txt"), "changed\n").expect("write");
    std::fs::create_dir_all(repo.path().join(".aid")).expect("mkdir");
    std::fs::write(repo.path().join(".aid/state.toml"), "health = 1\n").expect("write state");
    let store = Store::open_memory().expect("store");
    let mut task = task("t-salvage-aid-dir", Some(repo.path().display().to_string()));
    task.status = TaskStatus::Running;
    store.insert_task(&task).expect("insert");

    crate::task_lifecycle::mark_failed(&store, &task.id).expect("fail");

    let committed = git_stdout(
        repo.path(),
        &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"],
    );
    assert!(committed.lines().any(|line| line == "base.txt"), "got: {committed}");
    assert!(!committed.lines().any(|line| line == ".aid/state.toml"), "got: {committed}");
    // The file stays on disk (untouched), just never staged/committed by aid.
    assert!(repo.path().join(".aid/state.toml").exists());
}

#[test]
fn salvage_error_does_not_change_failed_status() {
    let (_home, _guard) = isolated_home();
    let not_repo = tempfile::tempdir().expect("tempdir");
    let store = Store::open_memory().expect("store");
    let mut task = task("t-salvage-error", Some(not_repo.path().display().to_string()));
    task.status = TaskStatus::Running;
    store.insert_task(&task).expect("insert");

    crate::task_lifecycle::mark_failed(&store, &task.id).expect("fail");

    let loaded = store
        .get_task(task.id.as_str())
        .expect("load")
        .expect("task");
    assert_eq!(loaded.status, TaskStatus::Failed);
}
