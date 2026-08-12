// Tests for `aid retry` supersede behavior and partial-work git helpers.
// Covers: same-task live lease supersede, rival refusal, and terminal
// self-holder refusal (message must not suggest separate worktree names).
// Deps: supersede_live_holder, stop, Store, tempfile, git.
use super::{reset_dirty_worktree, save_partial_work, supersede_live_holder};
use crate::paths::AidHomeGuard;
use crate::store::Store;
use crate::test_subprocess;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use tempfile::TempDir;

// In-memory store + isolated AID_HOME so `aid stop` bookkeeping never touches
// the real ~/.aid during supersede tests. The guard must live for the whole
// test, so it is returned and held by the caller.
fn isolated_store() -> (Arc<Store>, AidHomeGuard) {
    let home = TempDir::new().unwrap();
    let guard = AidHomeGuard::set(home.path());
    (Arc::new(Store::open_memory().unwrap()), guard)
}

fn make_task(id: &str, status: TaskStatus) -> Task {
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
        repo_path: None, project_id: None,
        worktree_path: None, effective_dir: None,
        worktree_branch: None,
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

// A lock whose owner_pid is our own (alive) process: the lease is live but no
// background spec exists, so `aid stop` can still terminate the task without
// killing the test process.
fn write_live_lock(dir: &Path, task_id: &str) {
    std::fs::write(
        dir.join(".aid-lock"),
        format!("version=1\ntask_id={task_id}\nowner_pid={}\nworker_pid=\n", std::process::id()),
    )
    .unwrap();
}

#[test]
fn supersede_stops_same_task_live_lease_before_retry() {
    let _permit = test_subprocess::acquire();
    let dir = TempDir::new().unwrap();
    let mut task = make_task("t-stalled-self", TaskStatus::Stalled);
    task.worktree_path = Some(dir.path().to_string_lossy().to_string());
    let (store, _guard) = isolated_store();
    store.insert_task(&task).unwrap();
    write_live_lock(dir.path(), "t-stalled-self");

    supersede_live_holder(&store, &task).unwrap();

    assert_eq!(
        store.get_task("t-stalled-self").unwrap().unwrap().status,
        TaskStatus::Stopped
    );
    assert!(!dir.path().join(".aid-lock").exists());
}

#[test]
fn supersede_refuses_genuinely_rival_task() {
    let _permit = test_subprocess::acquire();
    let dir = TempDir::new().unwrap();
    let mut task = make_task("t-retry-rival", TaskStatus::Failed);
    task.worktree_path = Some(dir.path().to_string_lossy().to_string());
    let (store, _guard) = isolated_store();
    store.insert_task(&task).unwrap();
    write_live_lock(dir.path(), "t-other-live");

    let err = supersede_live_holder(&store, &task).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("locked by task t-other-live"), "msg: {msg}");
    assert!(msg.contains("Use separate worktree names"), "msg: {msg}");
}

#[test]
fn supersede_terminal_self_holder_refuses_without_worktree_hint() {
    let _permit = test_subprocess::acquire();
    let dir = TempDir::new().unwrap();
    let mut task = make_task("t-done-self", TaskStatus::Done);
    task.worktree_path = Some(dir.path().to_string_lossy().to_string());
    let (store, _guard) = isolated_store();
    store.insert_task(&task).unwrap();
    write_live_lock(dir.path(), "t-done-self");

    let err = supersede_live_holder(&store, &task).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("already terminal"), "msg: {msg}");
    assert!(!msg.contains("separate worktree names"), "msg: {msg}");
}

#[test]
fn save_partial_work_commits_dirty_files() {
    let _permit = test_subprocess::acquire();
    let temp = tempfile::tempdir().unwrap();
    init_repo(temp.path());
    write_file(temp.path(), "tracked.txt", "base\n");
    git(temp.path(), &["add", "tracked.txt"]);
    git(temp.path(), &["commit", "-m", "initial"]);

    write_file(temp.path(), "tracked.txt", "changed\n");
    write_file(temp.path(), "new.txt", "new\n");

    save_partial_work(temp.path().to_str().unwrap(), "t-1234").unwrap();

    assert_eq!(head_message(temp.path()), "[aid] partial work from t-1234");
    assert!(git_stdout(temp.path(), &["status", "--porcelain"]).is_empty());
    assert_eq!(
        git_stdout(temp.path(), &["show", "--name-only", "--format=", "HEAD"]),
        "new.txt\ntracked.txt\n"
    );
}

#[test]
fn save_partial_work_excludes_aid_runtime_files() {
    let _permit = test_subprocess::acquire();
    let temp = tempfile::tempdir().unwrap();
    init_repo(temp.path());
    write_file(temp.path(), "tracked.txt", "base\n");
    git(temp.path(), &["add", "tracked.txt"]);
    git(temp.path(), &["commit", "-m", "initial"]);

    write_file(temp.path(), "tracked.txt", "changed\n");
    write_file(temp.path(), ".aid-lock", "pid=1234\n");
    write_file(temp.path(), ".aid-verify-deps-state", "fresh=1\n");

    save_partial_work(temp.path().to_str().unwrap(), "t-1234").unwrap();

    assert_eq!(head_message(temp.path()), "[aid] partial work from t-1234");
    let committed = git_stdout(temp.path(), &["show", "--name-only", "--format=", "HEAD"]);
    assert_eq!(committed, "tracked.txt\n", "got: {committed}");
    // Left on disk untouched, just never staged.
    assert!(temp.path().join(".aid-lock").exists());
    assert!(temp.path().join(".aid-verify-deps-state").exists());
}

#[test]
fn save_partial_work_excludes_aid_directory() {
    let _permit = test_subprocess::acquire();
    let temp = tempfile::tempdir().unwrap();
    init_repo(temp.path());
    write_file(temp.path(), "tracked.txt", "base\n");
    git(temp.path(), &["add", "tracked.txt"]);
    git(temp.path(), &["commit", "-m", "initial"]);

    write_file(temp.path(), "tracked.txt", "changed\n");
    std::fs::create_dir_all(temp.path().join(".aid")).unwrap();
    write_file(temp.path(), ".aid/state.toml", "health = 1\n");

    save_partial_work(temp.path().to_str().unwrap(), "t-1234").unwrap();

    assert_eq!(head_message(temp.path()), "[aid] partial work from t-1234");
    let committed = git_stdout(temp.path(), &["show", "--name-only", "--format=", "HEAD"]);
    assert_eq!(committed, "tracked.txt\n", "got: {committed}");
    // Left on disk untouched, just never staged.
    assert!(temp.path().join(".aid/state.toml").exists());
}

#[test]
fn reset_dirty_worktree_discards_dirty_files() {
    let _permit = test_subprocess::acquire();
    let temp = tempfile::tempdir().unwrap();
    init_repo(temp.path());
    write_file(temp.path(), "tracked.txt", "base\n");
    git(temp.path(), &["add", "tracked.txt"]);
    git(temp.path(), &["commit", "-m", "initial"]);

    write_file(temp.path(), "tracked.txt", "changed\n");
    write_file(temp.path(), "new.txt", "new\n");

    reset_dirty_worktree(temp.path().to_str().unwrap()).unwrap();

    assert_eq!(
        std::fs::read_to_string(temp.path().join("tracked.txt")).unwrap(),
        "base\n"
    );
    assert!(!temp.path().join("new.txt").exists());
    assert!(git_stdout(temp.path(), &["status", "--porcelain"]).is_empty());
    assert_eq!(head_message(temp.path()), "initial");
}

#[test]
fn clean_worktree_is_not_modified() {
    let _permit = test_subprocess::acquire();
    let temp = tempfile::tempdir().unwrap();
    init_repo(temp.path());
    write_file(temp.path(), "tracked.txt", "base\n");
    git(temp.path(), &["add", "tracked.txt"]);
    git(temp.path(), &["commit", "-m", "initial"]);

    save_partial_work(temp.path().to_str().unwrap(), "t-1234").unwrap();

    assert_eq!(head_message(temp.path()), "initial");
    assert!(git_stdout(temp.path(), &["status", "--porcelain"]).is_empty());
}

fn init_repo(path: &Path) {
    git(path, &["init"]);
    git(path, &["config", "user.name", "Test User"]);
    git(path, &["config", "user.email", "test@example.com"]);
}

fn write_file(path: &Path, name: &str, contents: &str) {
    std::fs::write(path.join(name), contents).unwrap();
}

fn head_message(path: &Path) -> String {
    git_stdout(path, &["log", "-1", "--pretty=%s"]).trim().to_string()
}

fn git_stdout(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap()
}

fn git(path: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success());
}
