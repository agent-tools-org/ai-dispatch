// Deterministic coverage of Done -> delivery failure during worker settlement.
use super::*;
use super::tests::make_task;
use crate::{background, paths};

fn pending_worker(id: &str, pid: u32) {
    let spec = serde_json::from_value(serde_json::json!({
        "task_id": id, "worker_pid": pid, "agent_name": "codex",
        "prompt": "deliver result", "retry": 0
    })).unwrap();
    background::save_spec(&spec).unwrap();
}

#[tokio::test]
async fn wait_does_not_accept_done_before_worker_settles() {
    let home = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(home.path());
    let store = Arc::new(Store::open_memory().unwrap());
    store.insert_task(&make_task("t-settling", TaskStatus::Done)).unwrap();
    pending_worker("t-settling", std::process::id());
    let outcome = wait_for_task_ids(&store, &["t-settling".into()], None, false,
        Some(Duration::from_millis(30))).await.unwrap();
    assert_eq!(outcome, WaitOutcome::TimedOut(vec!["t-settling".into()]));

    store.fail_completed_verify_gate("t-settling").unwrap();
    background::clear_spec("t-settling").unwrap();
    let outcome = wait_for_task_ids(&store, &["t-settling".into()], None, false, None).await.unwrap();
    assert_eq!(outcome, WaitOutcome::Failed(vec!["t-settling".into()]));
}

#[tokio::test]
async fn wait_accepts_success_only_after_worker_settles() {
    let home = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(home.path());
    let store = Arc::new(Store::open_memory().unwrap());
    store.insert_task(&make_task("t-settled", TaskStatus::Done)).unwrap();
    pending_worker("t-settled", std::process::id());
    assert_eq!(wait_for_task_ids(&store, &["t-settled".into()], None, false,
        Some(Duration::from_millis(30))).await.unwrap(),
        WaitOutcome::TimedOut(vec!["t-settled".into()]));
    background::clear_spec("t-settled").unwrap();
    assert_eq!(wait_for_task_ids(&store, &["t-settled".into()], None, false, None).await.unwrap(),
        WaitOutcome::Completed);
}

#[test]
fn discovery_includes_terminal_tasks_still_settling() {
    let home = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(home.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = make_task("t-group-settling", TaskStatus::Done);
    task.workgroup_id = Some("wg-settling".into());
    store.insert_task(&task).unwrap();
    pending_worker(task.id.as_str(), std::process::id());
    assert_eq!(current_running_ids(&store, None).unwrap(), vec![task.id.to_string()]);
    assert_eq!(current_running_ids(&store, Some("wg-settling")).unwrap(), vec![task.id.to_string()]);
    assert!(current_running_ids(&store, Some("wg-other")).unwrap().is_empty());
}

#[tokio::test]
async fn interrupted_settlement_cannot_report_success() {
    let home = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(home.path());
    let store = Arc::new(Store::open_memory().unwrap());
    store.insert_task(&make_task("t-interrupted", TaskStatus::Done)).unwrap();
    pending_worker("t-interrupted", u32::MAX);
    assert!(wait_for_task_ids(&store, &["t-interrupted".into()], None, false,
        Some(Duration::from_millis(30))).await.is_err());
}

#[tokio::test]
async fn live_worker_does_not_block_exit_on_await() {
    let home = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(home.path());
    let store = Arc::new(Store::open_memory().unwrap());
    store.insert_task(&make_task("t-awaiting", TaskStatus::AwaitingInput)).unwrap();
    pending_worker("t-awaiting", std::process::id());
    assert_eq!(wait_for_task_ids(&store, &["t-awaiting".into()], None, true,
        Some(Duration::from_millis(30))).await.unwrap(), WaitOutcome::Completed);
}
