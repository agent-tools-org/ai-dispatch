// Verify gate tests for skipped/error verification semantics.
// Covers configured verify bypasses, legitimate skip cases, and hint filtering.
// Deps: run verify wrapper, Store, worktree dependency state, tempfile.

use super::maybe_verify;
use crate::support::test_subprocess;
use crate::{
    store::Store,
    types::{TaskId, TaskStatus, VerifyStatus},
};
use std::sync::Arc;

#[path = "verify_gate_fixture.rs"]
mod fixture;
use fixture::task;

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
fn configured_missing_verify_fails_with_recorded_shell_output() {
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
    let error = store.latest_error(task_id.as_str()).unwrap();
    assert!(error.contains("Output: sh:"), "{error}");
    assert!(error.contains("missing-aid-verify-bin"), "{error}");
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
    std::fs::write(worktree.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    let dir_str = worktree.path().to_string_lossy().to_string();
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-rustc-vfail".to_string());
    crate::workspace::worktree::deps::prepare_worktree_dependencies(
        &store,
        &task_id,
        worktree.path(),
        worktree.path(),
        None,
        false,
        None,
        true,
        None,
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

#[path = "verify_gate_pending_tests.rs"]
mod pending;
