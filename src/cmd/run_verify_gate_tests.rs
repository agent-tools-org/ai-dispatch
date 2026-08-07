// Verify gate tests for skipped/error verification semantics.
// Covers configured verify bypasses, legitimate skip cases, and hint filtering.
// Deps: run verify wrapper, Store, worktree dependency state, tempfile.

use super::maybe_verify;
use crate::{
    store::Store,
    types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus},
};
use chrono::Local;

fn task(id: &str, status: TaskStatus, dir: Option<&str>, verify: Option<&str>) -> Task {
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
        repo_path: None,
        worktree_path: dir.map(str::to_string),
        worktree_branch: Some("fix/verify-gate".to_string()),
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: Some(10),
        requested_model: None, observed_model: None, attribution_source: None,
        cost_usd: None,
        exit_code: Some(0),
        created_at: Local::now(),
        completed_at: Some(Local::now()),
        verify: verify.map(str::to_string),
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
fn configured_verify_without_working_dir_is_a_legitimate_skip() {
    // A task dispatched without --dir has nothing to verify. The project's default verify
    // command is injected into every task in the repo, research runs included, so treating
    // this as a did-not-run failure would fail every dir-less task.
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-verify-no-dir".to_string());
    store
        .insert_task(&task(task_id.as_str(), TaskStatus::Done, None, Some("true")))
        .unwrap();

    maybe_verify(&store, &task_id, Some("true"), None, None);

    let task = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(task.verify_status, VerifyStatus::Skipped);
    assert_eq!(task.status, TaskStatus::Done);
    assert!(store.latest_error(task_id.as_str()).is_none());
}

#[test]
fn configured_verify_spawn_failure_fails_not_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let dir_str = dir.path().to_string_lossy().to_string();
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-verify-spawn-fail".to_string());
    store
        .insert_task(&task(task_id.as_str(), TaskStatus::Done, Some(&dir_str), Some("missing-aid-verify-bin")))
        .unwrap();

    maybe_verify(&store, &task_id, Some("missing-aid-verify-bin"), Some(&dir_str), None);

    let task = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(task.verify_status, VerifyStatus::Failed);
    assert_eq!(task.status, TaskStatus::Failed);
    assert!(store.latest_error(task_id.as_str()).unwrap().contains("Failed to run verify command"));
}

#[test]
fn no_configured_verify_leaves_done_task_successful() {
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-no-verify".to_string());
    store
        .insert_task(&task(task_id.as_str(), TaskStatus::Done, None, None))
        .unwrap();

    maybe_verify(&store, &task_id, None, None, None);

    let task = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(task.verify_status, VerifyStatus::Skipped);
    assert_eq!(task.status, TaskStatus::Done);
    assert_eq!(task.exit_code, Some(0));
}

#[test]
fn auto_verify_without_project_file_remains_legitimate_skip() {
    let dir = tempfile::tempdir().unwrap();
    let dir_str = dir.path().to_string_lossy().to_string();
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-no-project".to_string());
    store
        .insert_task(&task(task_id.as_str(), TaskStatus::Done, Some(&dir_str), Some("auto")))
        .unwrap();

    maybe_verify(&store, &task_id, Some("auto"), Some(&dir_str), None);

    let task = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(task.verify_status, VerifyStatus::Skipped);
    assert_eq!(task.status, TaskStatus::Done);
    assert_eq!(task.exit_code, Some(0));
}

#[test]
fn rustc_error_does_not_emit_missing_npm_hint() {
    let worktree = tempfile::tempdir().unwrap();
    let dir_str = worktree.path().to_string_lossy().to_string();
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-rustc-vfail".to_string());
    crate::worktree_deps::prepare_worktree_dependencies(
        &store,
        &task_id,
        worktree.path(),
        worktree.path(),
        None,
        false,
        None,
        true,
        Some("fix/verify-gate"),
    )
    .unwrap();
    let script = worktree.path().join("rustc-error.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\necho 'error[E0308]: mismatched types' >&2\nexit 1\n",
    )
    .unwrap();
    store
        .insert_task(&task(task_id.as_str(), TaskStatus::Done, Some(&dir_str), Some("auto")))
        .unwrap();

    maybe_verify(
        &store,
        &task_id,
        Some(&format!("sh {}", script.display())),
        Some(&dir_str),
        None,
    );

    let events = store.get_events(task_id.as_str()).unwrap();
    assert!(!events.iter().any(|event| {
        event.detail.contains("verify likely failed because dependencies weren't installed")
    }));
}

#[test]
fn read_only_task_skips_configured_verify() {
    let dir = tempfile::tempdir().unwrap();
    let dir_str = dir.path().to_string_lossy().to_string();
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-readonly-skip".to_string());
    let mut t = task(task_id.as_str(), TaskStatus::Done, Some(&dir_str), Some("false"));
    t.read_only = true;
    store.insert_task(&t).unwrap();

    maybe_verify(&store, &task_id, Some("false"), Some(&dir_str), None);

    let loaded = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(loaded.verify_status, VerifyStatus::Skipped);
    assert_eq!(loaded.status, TaskStatus::Done);
    assert!(store.latest_error(task_id.as_str()).is_none());
}

#[test]
fn empty_diff_skips_configured_verify() {
    let repo = tempfile::tempdir().unwrap();
    let dir = repo.path();
    assert!(std::process::Command::new("git")
        .args(["-C", &dir.to_string_lossy(), "init", "-b", "main"])
        .status()
        .unwrap()
        .success());
    assert!(std::process::Command::new("git")
        .args(["-C", &dir.to_string_lossy(), "config", "user.email", "t@example.com"])
        .status()
        .unwrap()
        .success());
    assert!(std::process::Command::new("git")
        .args(["-C", &dir.to_string_lossy(), "config", "user.name", "t"])
        .status()
        .unwrap()
        .success());
    std::fs::write(dir.join("file.txt"), "hello\n").unwrap();
    assert!(std::process::Command::new("git")
        .args(["-C", &dir.to_string_lossy(), "add", "file.txt"])
        .status()
        .unwrap()
        .success());
    assert!(std::process::Command::new("git")
        .args(["-C", &dir.to_string_lossy(), "commit", "-m", "init"])
        .status()
        .unwrap()
        .success());
    let dir_str = dir.to_string_lossy().to_string();
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-empty-skip".to_string());
    store
        .insert_task(&task(task_id.as_str(), TaskStatus::Done, Some(&dir_str), Some("false")))
        .unwrap();

    maybe_verify(&store, &task_id, Some("false"), Some(&dir_str), None);

    let loaded = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(loaded.verify_status, VerifyStatus::Skipped);
    assert_eq!(loaded.status, TaskStatus::Done);
    assert!(store.latest_error(task_id.as_str()).is_none());
}

#[test]
fn genuine_verify_failure_still_fails_task() {
    let dir = tempfile::tempdir().unwrap();
    let dir_str = dir.path().to_string_lossy().to_string();
    // Non-git dir so empty-diff skip does not fire; command still fails.
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-real-vfail".to_string());
    store
        .insert_task(&task(task_id.as_str(), TaskStatus::Done, Some(&dir_str), Some("false")))
        .unwrap();

    maybe_verify(&store, &task_id, Some("false"), Some(&dir_str), None);

    let loaded = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(loaded.verify_status, VerifyStatus::Failed);
    assert_eq!(loaded.status, TaskStatus::Failed);
}
