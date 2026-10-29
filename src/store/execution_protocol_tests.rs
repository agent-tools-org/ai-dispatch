// Store-level protocol regressions for revisions, settlement and insertion guards.
// Uses in-memory SQLite and file-backed reopening to cover durable facts.
use super::*;
use crate::types::{DispatchOrigin, TaskStatus, AgentKind};

fn task(id: &str) -> crate::types::Task {
    let mut task = crate::cost::test_support::task(AgentKind::Codex, 0, None);
    task.id = crate::types::TaskId(id.into());
    task.status = TaskStatus::Pending;
    task.requested_model = Some("exact-model".into());
    task
}

pub(super) fn policy() -> ExecutionPolicy {
    ExecutionPolicy {
        name: "caller-owned".into(),
        version: 1,
        agent: "codex".into(),
        model: "exact-model".into(),
        accepted_at: "2026-10-29T10:00:00Z".into(),
    }
}

#[test]
fn caller_owned_insert_is_atomic_and_starts_at_revision_one() {
    let store = Store::open_memory().expect("store");
    let task = task("t-owned");
    store
        .insert_task_with_policy(&task, DispatchOrigin::Automatic, Some(&policy()))
        .expect("insert");
    let facts = store.execution_facts("t-owned").expect("facts").expect("row");
    assert_eq!(facts.revision, 1);
    assert_eq!(facts.policy, Some(policy()));
    assert_eq!(facts.settlement.expect("worker").state, "pending");
    assert!(
        store
            .insert_task_with_policy(&task, DispatchOrigin::Automatic, None)
            .is_err()
    );
    assert!(
        store
            .is_caller_owned("t-owned")
            .expect("policy survived conflict")
    );
}

#[test]
fn caller_owned_parent_refuses_every_origin_and_direct_insert() {
    let store = Store::open_memory().expect("store");
    store
        .insert_task_with_policy(&task("t-parent"), DispatchOrigin::Automatic, Some(&policy()))
        .expect("parent");
    for origin in [DispatchOrigin::Automatic, DispatchOrigin::ExplicitRetry] {
        let mut child = task("t-child");
        child.parent_task_id = Some("t-parent".into());
        assert!(store.insert_task_with_origin(&child, origin).is_err());
        assert!(store.get_task("t-child").expect("query").is_none());
    }
    assert!(
        store
            .db()
            .execute(
                "INSERT INTO tasks (id, agent, prompt, status, created_at, parent_task_id)
         VALUES ('t-direct', 'codex', 'prompt', 'pending', 'now', 't-parent')",
                [],
            )
            .is_err()
    );
}

#[test]
fn caller_owned_revision_tracks_every_policy_write_and_worker_change() {
    let store = Store::open_memory().expect("store");
    store.insert_task(&task("t-revision")).expect("insert");
    let revision = || {
        store
            .execution_facts("t-revision")
            .expect("facts")
            .expect("row")
            .revision
    };
    assert_eq!(revision(), 1);
    store
        .update_task_status("t-revision", TaskStatus::Running)
        .expect("status");
    assert_eq!(revision(), 2);
    store
        .observe_worker_state("t-revision", "running", Some(42), None)
        .expect("worker");
    assert_eq!(revision(), 3);
    store
        .observe_worker_state("t-revision", "running", Some(42), None)
        .expect("unchanged");
    assert_eq!(revision(), 3);
    store
        .db()
        .execute(
            "UPDATE tasks SET execution_policy = NULL WHERE id = 't-revision'",
            [],
        )
        .expect("policy write");
    assert_eq!(revision(), 4);
    store
        .settle_worker("t-revision", "completed", None)
        .expect("settled");
    assert_eq!(revision(), 5);
    store
        .settle_worker("t-revision", "lost", None)
        .expect("idempotent settlement");
    assert_eq!(revision(), 5);
    assert_eq!(
        store
            .execution_facts("t-revision")
            .expect("facts")
            .expect("row")
            .settlement
            .expect("worker")
            .outcome
            .as_deref(),
        Some("completed")
    );
}

#[test]
fn caller_owned_revision_and_settlement_survive_reopen() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("aid.db");
    let store = Store::open(&path).expect("store");
    store
        .insert_task_with_policy(&task("t-durable"), DispatchOrigin::Automatic, Some(&policy()))
        .expect("insert");
    store
        .settle_worker("t-durable", "launch_failed", Some("spawn failed"))
        .expect("settled");
    let revision = store
        .execution_facts("t-durable")
        .expect("facts")
        .expect("row")
        .revision;
    drop(store);
    let store = Store::open(&path).expect("reopen");
    let facts = store.execution_facts("t-durable").expect("facts").expect("row");
    assert_eq!(facts.revision, revision);
    assert_eq!(facts.policy, Some(policy()));
    assert_eq!(
        facts.settlement.expect("settled").outcome.as_deref(),
        Some("launch_failed")
    );
}
