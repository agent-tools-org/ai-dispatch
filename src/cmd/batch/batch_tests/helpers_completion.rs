// Tests for batch completion polling and summary formatting.
// Exports: (tests only)
// Deps: super::shared + batch_helpers
use super::shared::{make_task, seed_task};
use crate::runtime::background::{BackgroundRunSpec, save_spec};
use crate::store::Store;

use crate::types::{AgentKind, TaskStatus};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::super::batch_helpers::batch_summary;
use super::super::batch_types::BatchTaskOutcome;

#[test]
fn reconcile_and_poll_completed_tasks_marks_zombies_failed() {
    let _temp = crate::support::test_env::TestHome::with_dirs();
    let store = Arc::new(Store::open_memory().unwrap());
    store
        .insert_task(&super::shared::make_stored_task(
            "t-zombie",
            AgentKind::Codex,
            TaskStatus::Running,
        ))
        .unwrap();
    save_spec(&BackgroundRunSpec {
        task_id: "t-zombie".to_string(),
        worker_pid: Some(999_999),
        dir: Some(".".to_string()),
        budget: false,
        max_duration_mins: None,
        max_duration_secs: None,
        idle_timeout_secs: None,
        interactive: true,
        on_done: None,
        env: None,
        agent_pid: None,
        agent_started: None,
        audit_report_mode: false,
        pre_task_dirty_paths: None,
        foreground: false,
    })
    .unwrap();
    let mut active = vec![(0, "t-zombie".to_string())];
    let completed =
        super::super::batch_dispatch::reconcile_and_poll_completed_tasks(&store, &mut active)
            .unwrap();
    assert!(active.is_empty());
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].task_id, "t-zombie");
    assert_eq!(completed[0].outcome, BatchTaskOutcome::Failed);
}

#[test]
fn batch_summary_formats_cost_and_time() {
    let store = Store::open_memory().unwrap();
    let task_ids = vec!["t-1".to_string(), "t-2".to_string(), "t-3".to_string()];
    seed_task(&store, "t-1", TaskStatus::Done, Some(12.125));
    seed_task(&store, "t-2", TaskStatus::Failed, Some(8.195));
    seed_task(&store, "t-3", TaskStatus::Skipped, None);

    let summary = batch_summary(
        &[
            BatchTaskOutcome::Done,
            BatchTaskOutcome::Failed,
            BatchTaskOutcome::Skipped,
        ],
        &task_ids,
        &[
            make_task("first-task", false, None),
            make_task("second-task", false, None),
            make_task("third-task", false, None),
        ],
        &store,
        Instant::now() - Duration::from_secs(83),
        None,
    );

    assert_eq!(
        summary,
        "[batch] 1/3 done, 1 failed, 1 skipped. Cost: $20.32. Time: 1m 23s\n[batch] Failed: t-2 (second-task)"
    );
}

#[test]
fn batch_summary_skips_zero_cost_and_uses_seconds_under_minute() {
    let store = Store::open_memory().unwrap();
    let task_ids = vec!["t-1".to_string(), "t-2".to_string()];
    seed_task(&store, "t-1", TaskStatus::Done, None);
    seed_task(&store, "t-2", TaskStatus::Done, Some(0.0));

    let summary = batch_summary(
        &[BatchTaskOutcome::Done, BatchTaskOutcome::Done],
        &task_ids,
        &[
            make_task("first-task", false, None),
            make_task("second-task", false, None),
        ],
        &store,
        Instant::now() - Duration::from_secs(42),
        None,
    );

    assert_eq!(summary, "[batch] 2/2 done, 0 failed, 0 skipped. Time: 42s");
}
