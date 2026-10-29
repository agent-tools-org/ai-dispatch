// Preparation failures must settle their claimed task with the actual error.
// Exercises both prompt construction and worktree setup before launch.
use super::*;

#[tokio::test]
async fn prompt_failure_records_preparation_error_without_launch() {
    let temp = crate::support::test_env::TestHome::new();
    let store = Arc::new(Store::open_memory().expect("store"));
    let id = TaskId("t-prompt-preparation".into());
    let args = RunArgs {
        agent_name: "codex".into(),
        prompt: "Inspect a concrete preparation failure".into(),
        existing_task_id: Some(id.clone()),
        dry_run: true,
        template: Some("missing-diagnostics-template".into()),
        dir: Some(temp.path().to_string_lossy().into_owned()),
        ..Default::default()
    };
    let err = run(Arc::clone(&store), args)
        .await
        .expect_err("missing template");
    let row = store
        .get_task(id.as_str())
        .expect("get")
        .expect("claimed task");
    assert_eq!(row.status, TaskStatus::Failed);
    assert_eq!(row.pending_reason.as_deref(), Some("preparation"));
    let events = store.get_events(id.as_str()).expect("events");
    assert!(
        events
            .iter()
            .any(|event| event.detail == format!("{err:#}"))
    );
    assert!(row.resolved_prompt.is_none());
    assert!(
        crate::runtime::background::load_spec_if_exists(id.as_str())
            .expect("spec")
            .is_none()
    );
}

#[test]
fn worktree_failure_records_preparation_error_after_claim() {
    let temp = crate::support::test_env::TestHome::new();
    let store = Arc::new(Store::open_memory().expect("store"));
    let id = TaskId("t-worktree-preparation".into());
    let mut args = RunArgs {
        agent_name: "codex".into(),
        prompt: "Inspect a concrete preparation failure".into(),
        existing_task_id: Some(id.clone()),
        dry_run: true,
        worktree: Some("fix/preparation".into()),
        dir: Some(temp.path().to_string_lossy().into_owned()),
        ..Default::default()
    };
    assert!(
        super::super::run_dispatch_prepare::prepare_dispatch_with(&store, &mut args, |_| true,)
            .is_err()
    );
    let row = store
        .get_task(id.as_str())
        .expect("get")
        .expect("claimed task");
    assert_eq!(row.status, TaskStatus::Failed);
    assert_eq!(row.pending_reason.as_deref(), Some("preparation"));
    assert!(
        store
            .get_events(id.as_str())
            .expect("events")
            .iter()
            .any(|event| event.detail.contains("Not a git repository"))
    );
}

#[test]
fn error_immediately_after_insertion_is_settled_and_preserved() {
    let temp = crate::support::test_env::TestHome::new();
    let store = Arc::new(Store::open_memory().expect("store"));
    store
        .db()
        .execute_batch(
            "CREATE TRIGGER fail_profile BEFORE UPDATE OF declared_difficulty ON tasks
         BEGIN SELECT RAISE(ABORT, 'preparation profile write failed'); END;",
        )
        .expect("inject failure");
    let id = TaskId("t-claim-preparation".into());
    let mut args = RunArgs {
        agent_name: "codex".into(),
        prompt: "Inspect a concrete preparation failure".into(),
        existing_task_id: Some(id.clone()),
        dry_run: true,
        dir: Some(temp.path().to_string_lossy().into_owned()),
        ..Default::default()
    };
    let result =
        super::super::run_dispatch_prepare::prepare_dispatch_with(&store, &mut args, |_| true);
    assert!(result.is_err());
    let row = store
        .get_task(id.as_str())
        .expect("get")
        .expect("claimed task");
    assert_eq!(row.status, TaskStatus::Failed);
    assert_eq!(row.pending_reason.as_deref(), Some("preparation"));
    let events = store.get_events(id.as_str()).expect("events");
    assert!(
        events
            .iter()
            .any(|event| event.detail.contains("preparation profile write failed"))
    );
    super::super::run_dispatch_prepare::record_error(
        &store,
        &id,
        &anyhow::anyhow!("later inferred reason"),
    )
    .expect("terminal task unchanged");
    assert_eq!(
        store.get_events(id.as_str()).expect("events").len(),
        events.len()
    );
}

fn prepare_pending_dispatch(
    store: &Arc<Store>,
    dir: &std::path::Path,
    id: &TaskId,
) -> crate::types::Task {
    let mut args = RunArgs {
        agent_name: "codex".into(),
        prompt: "Implement a concrete focused change".into(),
        existing_task_id: Some(id.clone()),
        dry_run: true,
        dir: Some(dir.to_string_lossy().into_owned()),
        ..Default::default()
    };
    super::super::run_dispatch_prepare::prepare_dispatch_with(store, &mut args, |_| true)
        .expect("prepare pending dispatch");
    store.get_task(id.as_str()).expect("get").expect("row")
}

fn record_agent_exit(store: &Store, id: &TaskId) {
    assert!(crate::task::lifecycle::mark_running(store, id).expect("start"));
    let event = crate::types::TaskEvent::new(
        id.clone(),
        crate::types::EventKind::Error,
        "agent exited 17".into(),
    );
    let completion = crate::store::TaskCompletionUpdate {
        exit_code: Some(17),
        ..TaskCompletionUpdate::new(id.as_str(), TaskStatus::Failed, 10)
    };
    assert!(
        crate::task::lifecycle::complete_task_atomic(store, completion, &event)
            .expect("fail agent")
    );
}

#[test]
fn launched_failure_preserves_exit_error_in_board_and_batch_summary() {
    let temp = crate::support::test_env::TestHome::new();
    let store = Arc::new(Store::open_memory().expect("store"));
    let id = TaskId("t-launched-failure".into());
    let pending = prepare_pending_dispatch(&store, temp.path(), &id);
    assert_eq!(pending.status, TaskStatus::Pending);
    record_agent_exit(&store, &id);
    let row = store.get_task(id.as_str()).expect("get").expect("row");
    let board = crate::ui::board::render_board(&[row.clone()], &store).expect("board");
    let summary = crate::cmd::batch::completion_progress_label(
        &store,
        id.as_str(),
        crate::cmd::batch::BatchTaskOutcome::Failed,
    )
    .expect("batch summary");
    assert!(board.contains("agent exited 17"), "{board}");
    assert_eq!(summary, "FAIL (exit 17)");
    assert!(!board.contains("preparation"));
    assert!(row.pending_reason.is_none());
}

#[test]
fn pending_dispatch_without_recorded_reason_uses_reaper_inference() {
    let temp = crate::support::test_env::TestHome::new();
    let store = Arc::new(Store::open_memory().expect("store"));
    let id = TaskId("t-pending-inference".into());
    let row = prepare_pending_dispatch(&store, temp.path(), &id);
    assert_eq!(row.status, TaskStatus::Pending);
    assert!(row.pending_reason.is_none());
    crate::routing::rate_limit::mark_rate_limited(
        &crate::types::AgentKind::Codex,
        None,
        &format!(
            "You've hit your usage limit. try again at {}.",
            crate::routing::rate_limit::test_future_recovery_time()
        ),
    );
    assert!(
        crate::runtime::background::fail_stale_pending_task(
            &store,
            &row,
            chrono::Local::now(),
            601,
        )
        .expect("reap pending")
    );
    let failed = store.get_task(id.as_str()).expect("get").expect("row");
    assert_eq!(failed.pending_reason.as_deref(), Some("rate_limited"));
    assert!(
        store
            .latest_error(id.as_str())
            .expect("error")
            .contains("reason: rate_limited")
    );
}
