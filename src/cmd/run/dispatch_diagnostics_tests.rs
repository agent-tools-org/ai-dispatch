// Preparation failures must settle their claimed task with the actual error.
// Exercises both prompt construction and worktree setup before launch.
use super::*;

#[tokio::test]
async fn prompt_failure_records_preparation_error_without_launch() {
    let temp = tempfile::tempdir().expect("home");
    let _home = crate::support::paths::AidHomeGuard::set(temp.path());
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
    let temp = tempfile::tempdir().expect("home");
    let _home = crate::support::paths::AidHomeGuard::set(temp.path());
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
    let temp = tempfile::tempdir().expect("home");
    let _home = crate::support::paths::AidHomeGuard::set(temp.path());
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
