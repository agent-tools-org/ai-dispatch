// End-to-end regression test for routing a real failing Cargo test through verify.
// Exports: no production API.
// Deps: run::maybe_verify, Store, task types, tempfile.

use super::maybe_verify;
use crate::{store::Store, types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus}};
use chrono::Local;

fn task(id: &str, dir: &str) -> Task {
    Task {
        id: TaskId(id.to_string()), agent: AgentKind::Codex, custom_agent_name: None,
        prompt: "prompt".to_string(), resolved_prompt: None, category: None,
        status: TaskStatus::Done, parent_task_id: None, workgroup_id: None,
        caller_kind: None, caller_session_id: None, agent_session_id: None,
        repo_path: None, project_id: None, worktree_path: Some(dir.to_string()), effective_dir: None,
        worktree_branch: Some("fix/verify-gate".to_string()), final_head_sha: None,
        final_branch: None, start_sha: None, log_path: None, output_path: None,
        tokens: None, prompt_tokens: None, duration_ms: Some(10), requested_model: None,
        observed_model: None, attribution_source: None, cost_usd: None, exit_code: Some(0),
        created_at: Local::now(), completed_at: Some(Local::now()), verify: Some("cargo test".to_string()),
        verify_status: VerifyStatus::Skipped, pending_reason: None, read_only: false,
        budget: false, audit_verdict: None, audit_report_path: None, delivery_assessment: None,
    }
}

#[test]
fn genuine_cargo_test_failure_remains_broken() {
    let _permit = crate::test_subprocess::acquire();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Cargo.toml"),
        "[package]\nname = \"verify-failure\"\nversion = \"0.1.0\"\nedition = \"2021\"\n").unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/lib.rs"),
        "#[test]\nfn deliberately_fails() { assert!(false); }\n").unwrap();
    let dir_str = dir.path().to_string_lossy().to_string();
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-cargo-test-failed".to_string());
    store.insert_task(&task(task_id.as_str(), &dir_str)).unwrap();

    maybe_verify(&store, &task_id, Some("cargo test"), Some(&dir_str), None);

    let loaded = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(loaded.verify_status, VerifyStatus::Failed);
    assert_eq!(loaded.status, TaskStatus::Failed);
}

/// The Cargo route must not widen the infrastructure verdict to every verify
/// command. A non-Cargo command that exits nonzero is a broken delivery, and
/// stays one even when its output mentions permissions — only Cargo's own
/// target-path diagnostic may downgrade a task to Unverified.
#[test]
fn non_cargo_verify_failure_remains_broken() {
    let _permit = crate::test_subprocess::acquire();
    let dir = tempfile::tempdir().unwrap();
    let dir_str = dir.path().to_string_lossy().to_string();
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-non-cargo-verify-failed".to_string());
    store.insert_task(&task(task_id.as_str(), &dir_str)).unwrap();

    // split_command() splits on whitespace with no shell quoting, so the payload
    // has to live in a script rather than an inline `sh -c` string.
    let script = dir.path().join("fail.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\necho 'error: error writing dependencies to `/not/the/target/deps/foo.d`: Operation not permitted (os error 1)' >&2\nexit 1\n",
    )
    .unwrap();
    let command = format!("sh {}", script.display());

    maybe_verify(&store, &task_id, Some(&command), Some(&dir_str), None);

    // The diagnostic really was emitted — without this the assertions below
    // would pass on a command that never printed anything.
    let events = store.get_events(task_id.as_str()).unwrap();
    assert!(
        events.iter().any(|event| event.detail.contains("Operation not permitted")),
        "verify output never carried the EPERM text, so this test proves nothing"
    );
    assert_eq!(loaded_status(&store, &task_id), (VerifyStatus::Failed, TaskStatus::Failed));
}

fn loaded_status(store: &Store, task_id: &TaskId) -> (VerifyStatus, TaskStatus) {
    let task = store.get_task(task_id.as_str()).unwrap().unwrap();
    (task.verify_status, task.status)
}
