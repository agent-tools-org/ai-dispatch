// A failed launch settlement write must not suppress the existing preparation failure path.
// The task status and original error event remain authoritative even when settlement storage fails.
use super::*;

#[test]
fn caller_owned_preparation_settlement_failure_still_fails_pending_row() {
    let (_home, store) = crate::support::test_env::TestHome::with_dirs().shared_store();
    let task = Task::test_fixture(TaskId("t-preparation-settlement".into()), TaskStatus::Pending);
    store.insert_task(&task).expect("task");
    store
        .db()
        .execute_batch(
            "CREATE TRIGGER reject_settlement BEFORE UPDATE OF worker_settlement ON tasks
         BEGIN SELECT RAISE(ABORT, 'settlement unavailable'); END;",
        )
        .expect("inject settlement failure");
    record_error(&store, &task.id, &anyhow::anyhow!("original preparation failure"))
        .expect("existing status and error cleanup still run");
    let row = store.get_task(task.id.as_str()).expect("task").expect("row");
    assert_eq!(row.status, TaskStatus::Failed);
    assert_eq!(row.pending_reason.as_deref(), Some("preparation"));
    assert_eq!(
        store.latest_error(task.id.as_str()).as_deref(),
        Some("original preparation failure")
    );
    assert_ne!(
        store
            .execution_facts(task.id.as_str())
            .expect("facts")
            .expect("row")
            .settlement
            .expect("worker state")
            .state,
        "settled"
    );
}

#[test]
fn explicit_metered_dispatch_records_prepaid_notice_without_blocking() {
    let home = crate::support::test_env::TestHome::with_dirs();
    let _cache = crate::routing::quota::live::CacheDirGuard::set(home.path());
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Cursor, AgentKind::OpenCode]);
    let store = Arc::new(Store::open_memory().expect("store"));
    let dir = tempfile::tempdir().expect("dir");
    let id = TaskId("t-metered-notice".into());
    let mut args = RunArgs {
        agent_name: "opencode".into(),
        prompt: "Investigate the selection scoring policy.".into(),
        dir: Some(dir.path().to_string_lossy().into()),
        existing_task_id: Some(id.clone()),
        ..Default::default()
    };
    super::super::prepare_dispatch_with(&store, &mut args, |_| true).expect("dispatch allowed");
    let events = store.get_events(id.as_str()).expect("events");
    let notices: Vec<_> = events.iter().filter(|e| e.detail.starts_with("metered route")).collect();
    assert_eq!(notices.len(), 1);
    assert!(notices[0].detail.contains("cursor (usage unknown)"));
}
