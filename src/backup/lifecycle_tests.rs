// Backup attempt semantics: a settled task uploads once, unknown dispatch intent
// means no backup, one attempt per task even after a failure, a failed backup
// keeps the agent's error, and a reaper failure alone never backs up. Fake
// `gws` scripts stand in for the real binary.

use super::*;
use crate::paths::AidHomeGuard;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

fn task(id: &str, status: TaskStatus) -> Task {
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
        repo_path: None, project_id: Some("proj".to_string()),
        worktree_path: None, effective_dir: None,
        worktree_branch: None,
        final_head_sha: Some("0123456789abcdef".to_string()),
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

fn event(task_id: &str, kind: EventKind, detail: &str) -> TaskEvent {
    TaskEvent {
        task_id: TaskId(task_id.to_string()),
        timestamp: Local::now(),
        event_kind: kind,
        detail: detail.to_string(),
        metadata: None,
    }
}

/// A `gws` that logs every call and always fails, wired in through the global
/// `[backup.gdrive] binary` key so no PATH manipulation is needed.
fn failing_gws(home: &Path) -> std::path::PathBuf {
    fake_gws(home, "echo 'not signed in' >&2\nexit 2")
}

/// A `gws` that logs every call and answers every call with `{"id":"fake123"}`.
fn working_gws(home: &Path) -> std::path::PathBuf {
    fake_gws(home, "echo '{\"id\":\"fake123\"}'")
}

fn fake_gws(home: &Path, body: &str) -> std::path::PathBuf {
    let log = home.join("gws.log");
    let binary = home.join("gws");
    let script = format!("#!/bin/sh\necho \"$*\" >> '{}'\n{body}\n", log.display());
    fs::write(&binary, script).unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(
        crate::paths::config_path(),
        format!("[backup.gdrive]\nbinary = '{}'\n", binary.display()),
    )
    .unwrap();
    log
}

fn save_args(store: &Store, task_id: &str, args: crate::cmd::run::RunArgs) {
    store.update_task_dispatch_args(task_id, &args.dispatch_args_json().unwrap()).unwrap();
}

/// Forces the stored status (bypassing transition guards) and settles the task.
pub(super) fn settle(store: &Store, task_id: &str, status: TaskStatus) {
    store
        .db()
        .execute("UPDATE tasks SET status = ?1 WHERE id = ?2", rusqlite::params![status.as_str(), task_id])
        .unwrap();
    on_settled(store, task_id);
}

#[test]
fn settled_done_task_uploads_exactly_once() {
    let _permit = crate::test_subprocess::acquire();
    let home = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(home.path());
    let log = working_gws(home.path());
    let store = Store::open_memory().unwrap();
    store.insert_task(&task("t-done", TaskStatus::Done)).unwrap();
    save_args(&store, "t-done", crate::cmd::run::RunArgs { backup: Some("gdrive:audits".into()), ..Default::default() });

    on_settled(&store, "t-done");

    let calls = fs::read_to_string(&log).unwrap();
    assert_eq!(calls.lines().filter(|l| l.contains("--upload")).count(), 1, "{calls}");
    let events = store.get_events("t-done").unwrap();
    let markers: Vec<_> = events.iter().filter_map(|e| e.metadata.as_ref()?.get("backup")?.as_str()).collect();
    assert_eq!(markers, vec!["uploaded"], "{events:?}");
    assert!(store.backup_url("t-done").unwrap().is_some_and(|url| url.contains("fake123")));
    assert_eq!(store.get_task("t-done").unwrap().unwrap().status, TaskStatus::Done);
}

#[test]
fn task_failed_by_fail_active_execution_alone_is_not_backed_up() {
    let home = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(home.path());
    let log = working_gws(home.path());
    let store = Store::open_memory().unwrap();
    store.insert_task(&task("t-reaped", TaskStatus::Running)).unwrap();
    save_args(&store, "t-reaped", crate::cmd::run::RunArgs { backup: Some("gdrive".into()), ..Default::default() });

    assert!(crate::task_lifecycle::fail_active_execution(&store, "t-reaped").unwrap());

    assert_eq!(store.get_task("t-reaped").unwrap().unwrap().status, TaskStatus::Failed);
    assert!(!log.exists(), "gws must never run");
    assert!(!already_attempted(&store, "t-reaped"));
    assert!(store.get_events("t-reaped").unwrap().iter().all(|e| e.metadata.is_none()));
}

#[test]
fn task_without_saved_dispatch_args_is_never_backed_up() {
    let home = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(home.path());
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join(".git")).unwrap();
    fs::create_dir_all(repo.path().join(".aid")).unwrap();
    fs::write(repo.path().join(".aid/project.toml"), "[project]\nid = 'proj'\n[backup]\ntarget = 'nope'\n").unwrap();
    let store = Store::open_memory().unwrap();
    let mut failed = task("t-setup-fail", TaskStatus::Failed);
    failed.repo_path = Some(repo.path().display().to_string());
    store.insert_task(&failed).unwrap();

    // Setup failed before dispatch args were persisted: the project default
    // must not win over an unknown `--no-backup`.
    settle(&store, "t-setup-fail", TaskStatus::Failed);
    assert!(store.get_events("t-setup-fail").unwrap().is_empty());
    assert!(!already_attempted(&store, "t-setup-fail"));

    // The same task with its args persisted resolves the project target.
    save_args(&store, "t-setup-fail", crate::cmd::run::RunArgs::default());
    settle(&store, "t-setup-fail", TaskStatus::Failed);
    let events = store.get_events("t-setup-fail").unwrap();
    assert!(events.iter().any(|e| e.detail.contains("unknown backup target 'nope'")), "{events:?}");
}

#[test]
fn backup_runs_at_most_once_even_after_a_failed_attempt() {
    let home = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(home.path());
    let log = failing_gws(home.path());
    let store = Store::open_memory().unwrap();
    store.insert_task(&task("t-once", TaskStatus::Done)).unwrap();
    let args = crate::cmd::run::RunArgs { backup: Some("gdrive:audits".into()), ..Default::default() };
    save_args(&store, "t-once", args);

    settle(&store, "t-once", TaskStatus::Done);
    assert!(already_attempted(&store, "t-once"));
    settle(&store, "t-once", TaskStatus::Done);
    settle(&store, "t-once", TaskStatus::Failed);

    let calls = fs::read_to_string(&log).unwrap();
    assert_eq!(calls.lines().count(), 1, "one gws call for the whole task: {calls}");
    let events = store.get_events("t-once").unwrap();
    let attempts: Vec<_> = events.iter().filter(|e| e.detail.starts_with("Backup failed")).collect();
    assert_eq!(attempts.len(), 1, "{events:?}");
    assert!(attempts[0].detail.contains("not signed in"), "{}", attempts[0].detail);
    assert!(store.backup_url("t-once").unwrap().is_none());
    // The last settled status stands; backup never rewrites it.
    assert_eq!(store.get_task("t-once").unwrap().unwrap().status, TaskStatus::Failed);
}

#[test]
fn failed_backup_keeps_the_agent_error_as_latest_error() {
    let home = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(home.path());
    failing_gws(home.path());
    let store = Store::open_memory().unwrap();
    store.insert_task(&task("t-agent-err", TaskStatus::Failed)).unwrap();
    store.insert_event(&event("t-agent-err", EventKind::Error, "agent exited with code 1")).unwrap();
    save_args(&store, "t-agent-err", crate::cmd::run::RunArgs { backup: Some("gdrive".into()), ..Default::default() });

    settle(&store, "t-agent-err", TaskStatus::Failed);

    assert_eq!(store.latest_error("t-agent-err").as_deref(), Some("agent exited with code 1"));
    let events = store.get_events("t-agent-err").unwrap();
    let warning = events.iter().find(|e| e.detail.starts_with("Backup failed")).unwrap();
    assert_eq!(warning.event_kind, EventKind::Milestone);
    assert_eq!(warning.metadata.as_ref().unwrap()["backup"], "failed");
    assert_eq!(store.get_task("t-agent-err").unwrap().unwrap().status, TaskStatus::Failed);
}
