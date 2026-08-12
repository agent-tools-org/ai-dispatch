// Verify gate tests for skipped/error verification semantics.
// Covers configured verify bypasses, legitimate skip cases, and hint filtering.
// Deps: run verify wrapper, Store, worktree dependency state, tempfile.

use super::maybe_verify;
use crate::test_subprocess;
use crate::{
    store::Store,
    types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus},
};
use chrono::Local;
use std::sync::Arc;

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
        repo_path: None, project_id: None,
        worktree_path: dir.map(str::to_string), effective_dir: None,
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
fn configured_verify_spawn_failure_is_inconclusive() {
    let dir = tempfile::tempdir().unwrap();
    let dir_str = dir.path().to_string_lossy().to_string();
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-verify-spawn-fail".to_string());
    store
        .insert_task(&task(task_id.as_str(), TaskStatus::Done, Some(&dir_str), Some("missing-aid-verify-bin")))
        .unwrap();

    maybe_verify(&store, &task_id, Some("missing-aid-verify-bin"), Some(&dir_str), None);

    let task = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(task.verify_status, VerifyStatus::InfrastructureFailure);
    assert_eq!(task.status, TaskStatus::Done);
    assert!(store.latest_error(task_id.as_str()).is_none());
}

/// A completed verify process is governed by its exit status. AID must not
/// reinterpret failure text as an infrastructure exception.
#[cfg(unix)]
#[test]
fn nonzero_verify_is_failed_regardless_of_sccache_wording() {
    let _permit = test_subprocess::acquire();
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("sccache-death.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\necho 'sccache: encountered fatal error'\necho 'sccache: error: failed to spawn Command' >&2\nexit 1\n",
    )
    .unwrap();
    let dir_str = dir.path().to_string_lossy().to_string();
    let command = format!("sh {}", script.to_string_lossy());
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-verify-sccache".to_string());
    store
        .insert_task(&task(task_id.as_str(), TaskStatus::Done, Some(&dir_str), Some(&command)))
        .unwrap();

    maybe_verify(&store, &task_id, Some(&command), Some(&dir_str), None);

    let task = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(task.verify_status, VerifyStatus::Failed);
    assert_eq!(task.status, TaskStatus::Failed);
    assert!(store.latest_error(task_id.as_str()).is_some());
}

/// The mirror of the incident test, and the more dangerous direction: a false
/// infrastructure classification turns a broken change into an inconclusive one.
/// Tooling noise alongside a real compiler diagnostic must still fail the task.
#[cfg(unix)]
#[test]
fn sccache_noise_with_a_compiler_diagnostic_still_fails_the_task() {
    let _permit = test_subprocess::acquire();
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("sccache-and-error.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\necho 'sccache: encountered fatal error'\necho 'error[E0308]: mismatched types'\nexit 1\n",
    )
    .unwrap();
    let dir_str = dir.path().to_string_lossy().to_string();
    let command = format!("sh {}", script.to_string_lossy());
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-verify-sccache-diag".to_string());
    store
        .insert_task(&task(task_id.as_str(), TaskStatus::Done, Some(&dir_str), Some(&command)))
        .unwrap();

    maybe_verify(&store, &task_id, Some(&command), Some(&dir_str), None);

    let loaded = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(loaded.verify_status, VerifyStatus::Failed);
    assert_eq!(loaded.status, TaskStatus::Failed);
}

#[test]
fn prompt_prose_does_not_create_an_undeclared_verify_contract() {
    let dir = tempfile::tempdir().unwrap();
    let dir_str = dir.path().to_string_lossy().to_string();
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-verify-prompt-prose".to_string());
    let mut stored = task(task_id.as_str(), TaskStatus::Done, Some(&dir_str), Some("true"));
    stored.prompt = "Create a new file: guessed-from-prose.txt".to_string();
    store.insert_task(&stored).unwrap();

    maybe_verify(&store, &task_id, Some("true"), Some(&dir_str), None);

    let loaded = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(loaded.verify_status, VerifyStatus::Passed);
    assert_eq!(loaded.status, TaskStatus::Done);
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
fn empty_diff_still_runs_configured_verify_and_fails_on_broken_tree() {
    // Agent delivered nothing (clean tree, no commits ahead). That is delivery
    // assessment, not a verify skip — configured verify must still run and can
    // fail when the tree is already broken (`false` as stand-in).
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
    let task_id = TaskId("t-empty-still-verify".to_string());
    store
        .insert_task(&task(task_id.as_str(), TaskStatus::Done, Some(&dir_str), Some("false")))
        .unwrap();

    maybe_verify(&store, &task_id, Some("false"), Some(&dir_str), None);

    let loaded = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert!(loaded.verify_status.was_attempted());
    assert_eq!(loaded.verify_status, VerifyStatus::Failed);
    assert_eq!(loaded.status, TaskStatus::Failed);
}

#[test]
fn genuine_verify_failure_still_fails_task() {
    let dir = tempfile::tempdir().unwrap();
    let dir_str = dir.path().to_string_lossy().to_string();
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

#[cfg(unix)]
#[test]
fn configured_verify_records_pending_before_command_finishes() {
    let _permit = test_subprocess::acquire();
    let dir = tempfile::tempdir().unwrap();
    let dir_str = dir.path().to_string_lossy().to_string();
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-verify-pending-start".to_string());
    store
        .insert_task(&task(task_id.as_str(), TaskStatus::Done, Some(&dir_str), Some("sleep 1")))
        .unwrap();

    let verify_store = store.clone();
    let verify_id = task_id.clone();
    let handle = std::thread::spawn(move || {
        maybe_verify(&verify_store, &verify_id, Some("sleep 1"), Some(&dir_str), None);
    });

    let mut observed_pending = false;
    for _ in 0..50 {
        if store
            .get_task(task_id.as_str())
            .unwrap()
            .is_some_and(|loaded| loaded.verify_status == VerifyStatus::Pending)
        {
            observed_pending = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(observed_pending);
    handle.join().unwrap();
    assert_eq!(
        store.get_task(task_id.as_str()).unwrap().unwrap().verify_status,
        VerifyStatus::Passed
    );
}
