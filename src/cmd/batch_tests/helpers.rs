// Tests for batch helpers (path/workgroup/summary).
// Exports: (tests only)
// Deps: super::shared + batch_helpers
use super::shared::{make_task, seed_task};
use crate::paths::AidHomeGuard;
use crate::store::Store;
use crate::types::{TaskFilter, TaskStatus};
use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::super::batch_helpers::{batch_summary, ensure_batch_workgroup, resolve_batch_path};
use super::super::batch_types::BatchTaskOutcome;

#[test]
fn resolve_batch_path_uses_aid_batches_fallback() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(temp.path());

    let batches_dir = crate::paths::aid_dir().join("batches");
    fs::create_dir_all(&batches_dir).unwrap();
    let fallback = batches_dir.join("deploy.toml");
    fs::write(&fallback, "tasks = []\n").unwrap();

    let resolved = resolve_batch_path(std::path::Path::new("deploy.toml"));

    assert_eq!(resolved, fallback);
}

#[test]
fn ensure_batch_workgroup_reuses_existing_default_group() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(temp.path());
    let store = Store::open_memory().unwrap();
    let existing = store
        .create_workgroup("existing", "shared", Some("seed"), Some("wg-shared"))
        .unwrap();

    let (workgroup_id, shared_path) =
        ensure_batch_workgroup(&store, "batch", Some("wg-shared"), false).unwrap();
    let workgroups = store.list_workgroups().unwrap();

    assert_eq!(workgroup_id, existing.id.to_string());
    assert_eq!(shared_path, None);
    assert_eq!(workgroups.len(), 1);
}

#[test]
fn ensure_batch_workgroup_creates_missing_default_group() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(temp.path());
    let store = Store::open_memory().unwrap();

    let (workgroup_id, shared_path) =
        ensure_batch_workgroup(&store, "batch", Some("wg-custom"), false).unwrap();
    let workgroups = store.list_workgroups().unwrap();
    let workgroup = store.get_workgroup("wg-custom").unwrap().unwrap();

    assert_eq!(workgroup_id, "wg-custom");
    assert_eq!(shared_path, None);
    assert_eq!(workgroups.len(), 1);
    assert_eq!(workgroup.name, "batch");
}

#[test]
fn ensure_batch_workgroup_creates_shared_dir_when_enabled() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(temp.path());
    let store = Store::open_memory().unwrap();

    let (workgroup_id, shared_path) =
        ensure_batch_workgroup(&store, "batch", Some("wg-custom"), true).unwrap();

    assert_eq!(workgroup_id, "wg-custom");
    assert_eq!(
        shared_path,
        crate::shared_dir::shared_dir_path("wg-custom")
    );
    assert!(shared_path.as_ref().is_some_and(|path| path.is_dir()));
}

#[tokio::test]
async fn batch_group_flag_assigns_existing_workgroup() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());
    store
        .create_workgroup("existing", "shared", Some("seed"), Some("wg-shared"))
        .unwrap();

    let batch_file = temp.path().join("tasks.toml");
    fs::write(
        &batch_file,
        r#"
[[tasks]]
name = "first"
agent = "codex"
prompt = "first prompt"

[[tasks]]
name = "second"
agent = "codex"
prompt = "second prompt"
"#,
    )
    .unwrap();

    crate::cmd::batch::run(
        store.clone(),
        crate::cmd::batch::BatchArgs {
            file: batch_file.display().to_string(),
            vars: vec![],
            group: Some("wg-shared".to_string()),
            parallel: false,
            analyze: false,
            wait: false,
            dry_run: true,
            max_concurrent: None,
        },
    )
    .await
    .unwrap();

    let tasks = store.list_tasks(TaskFilter::All).unwrap();
    assert_eq!(tasks.len(), 2);
    assert!(tasks.iter().all(|task| task.workgroup_id.as_deref() == Some("wg-shared")));
}

#[test]
fn batch_summary_formats_cost_and_time() {
    let store = Store::open_memory().unwrap();
    let task_ids = vec!["t-1".to_string(), "t-2".to_string(), "t-3".to_string()];
    seed_task(&store, "t-1", TaskStatus::Done, Some(12.125));
    seed_task(&store, "t-2", TaskStatus::Failed, Some(8.195));
    seed_task(&store, "t-3", TaskStatus::Skipped, None);

    let summary = batch_summary(
        &[BatchTaskOutcome::Done, BatchTaskOutcome::Failed, BatchTaskOutcome::Skipped],
        &task_ids,
        &[
            make_task("first-task", false, None),
            make_task("second-task", false, None),
            make_task("third-task", false, None),
        ],
        &store,
        Instant::now() - Duration::from_secs(83),
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
        &[make_task("first-task", false, None), make_task("second-task", false, None)],
        &store,
        Instant::now() - Duration::from_secs(42),
    );

    assert_eq!(summary, "[batch] 2/2 done, 0 failed, 0 skipped. Time: 42s");
}
