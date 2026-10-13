// Success settlement regressions across agent exit, verification and user stop.
// Uses the existing completion timestamp to distinguish provisional success.
// Deps: task lifecycle, post-run lifecycle and an isolated in-memory Store.

use super::{stop, tests::make_task};
use crate::{
    cmd::run,
    paths,
    store::{Store, TaskCompletionUpdate},
    types::*,
};
use std::sync::Arc;

fn setup() -> (tempfile::TempDir, paths::AidHomeGuard, Arc<Store>, TaskId) {
    let home = tempfile::tempdir().expect("home");
    let guard = paths::AidHomeGuard::set(home.path());
    paths::ensure_dirs().expect("dirs");
    let store = Arc::new(Store::open_memory().expect("store"));
    let id = TaskId("t-settle-done".into());
    store
        .insert_task(&make_task(id.as_str(), TaskStatus::Running))
        .expect("task");
    crate::task_lifecycle::update_task_completion(
        &store,
        TaskCompletionUpdate {
            id: id.as_str(),
            status: TaskStatus::Done,
            tokens: Some(10),
            duration_ms: 100,
            observed_model: None,
            attribution_source: None,
            cost_usd: None,
            exit_code: Some(0),
        },
    )
    .expect("agent exit");
    (home, guard, store, id)
}

async fn settle(store: &Arc<Store>, id: &TaskId) {
    let bundle = run::PromptBundle {
        effective_prompt: "prompt".into(),
        context_files: vec![],
        prompt_tokens: 0,
        injected_memory_ids: vec![],
    };
    run::post_run_lifecycle(
        run::LifecycleMode::Background,
        store,
        id,
        &run::RunArgs::default(),
        AgentKind::Codex,
        "codex",
        None,
        None,
        None,
        None,
        &[],
        &bundle,
        TaskStatus::Done,
        None,
    )
    .await
    .expect("settlement");
}

#[tokio::test]
async fn settle_done_agent_success_is_provisional_until_checks_finish() {
    let (_home, _guard, store, id) = setup();
    let provisional = store.get_task(id.as_str()).expect("get").expect("task");
    assert_eq!(provisional.status, TaskStatus::Done);
    assert!(
        provisional.completed_at.is_none(),
        "agent exit must not settle success"
    );
    settle(&store, &id).await;
    let settled = store.get_task(id.as_str()).expect("get").expect("task");
    assert_eq!(settled.status, TaskStatus::Done);
    assert!(settled.completed_at.is_some());
    assert_eq!(settled.duration_ms, Some(100));
    assert!(
        !store
            .update_task_status(id.as_str(), TaskStatus::Failed)
            .expect("late failure")
    );
    assert_eq!(
        store.get_task(id.as_str()).expect("get").expect("task").status,
        TaskStatus::Done
    );
}

#[test]
fn settle_done_stop_can_win_after_agent_exit_before_settlement() {
    let (_home, _guard, store, id) = setup();
    stop(&store, id.as_str()).expect("stop provisional success");
    let task = store.get_task(id.as_str()).expect("get").expect("task");
    assert_eq!(task.status, TaskStatus::Stopped);
    assert!(task.completed_at.is_some());
    assert!(task.duration_ms.is_some());
    let events = store.get_events(id.as_str()).expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].detail, "Task stopped by user");
}

#[test]
fn settle_done_provisional_success_still_fails_verification() {
    let (_home, _guard, store, id) = setup();
    store
        .update_verify_status(id.as_str(), VerifyStatus::Failed)
        .expect("verify result");
    crate::verify::enforce_verify_status(&store, &id);
    let task = store.get_task(id.as_str()).expect("get").expect("task");
    assert_eq!(task.status, TaskStatus::Failed);
    assert!(task.completed_at.is_some());
    assert!(task.duration_ms.is_some());
    assert_eq!(task.exit_code, Some(1));
}

#[test]
fn settle_done_worker_failure_can_win_before_success_settlement() {
    let (_home, _guard, store, id) = setup();
    assert!(
        crate::background::record_failure(&store, id.as_str(), "worker failed", "worker failed")
            .expect("worker failure")
    );
    let task = store.get_task(id.as_str()).expect("get").expect("task");
    assert_eq!(task.status, TaskStatus::Failed);
    assert!(task.completed_at.is_some());
    assert!(task.duration_ms.is_some());
    assert_eq!(store.get_events(id.as_str()).expect("events").len(), 1);
}

#[test]
fn settle_done_retry_tree_includes_provisional_success() {
    let (_home, _guard, store, id) = setup();
    super::stop_retry_tree(&store, id.as_str(), false).expect("stop retry tree");
    assert_eq!(
        store.get_task(id.as_str()).expect("get").expect("task").status,
        TaskStatus::Stopped
    );
    assert_eq!(store.get_events(id.as_str()).expect("events").len(), 1);
}
