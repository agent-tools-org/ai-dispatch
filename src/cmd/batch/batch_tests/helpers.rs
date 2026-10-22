// Tests for batch helpers (path/workgroup/summary).
// Exports: (tests only)
// Deps: super::shared + batch_helpers
use super::shared::seed_task;
use crate::batch;
use crate::store::Store;
use crate::support::paths::AidHomeGuard;
use crate::types::{TaskFilter, TaskStatus};
use std::fs;
use std::sync::Arc;
use std::time::Instant;

use super::super::batch_helpers::{batch_summary, ensure_batch_workgroup, resolve_batch_path};
use super::super::batch_types::{BatchDispatchResult, BatchTaskOutcome};
use super::super::{batch_wait_error, finalize_batch};
use crate::cmd::wait::WaitOutcome;

struct EnvVarGuard {
    key: &'static str,
    original: Option<String>,
}

impl EnvVarGuard {
    fn remove(key: &'static str) -> Self {
        let original = std::env::var(key).ok();
        unsafe { std::env::remove_var(key) };
        Self { key, original }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match self.original.as_deref() {
            Some(value) => unsafe { std::env::set_var(self.key, value) },
            None => unsafe { std::env::remove_var(self.key) },
        }
    }
}

#[test]
fn resolve_batch_path_uses_aid_batches_fallback() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(temp.path());

    let batches_dir = crate::support::paths::aid_dir().join("batches");
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
        crate::support::shared_dir::shared_dir_path("wg-custom")
    );
    assert!(shared_path.as_ref().is_some_and(|path| path.is_dir()));
}

#[tokio::test]
async fn batch_title_sets_auto_created_workgroup_name() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(temp.path());
    let _group_guard = EnvVarGuard::remove("AID_GROUP");
    let store = Arc::new(Store::open_memory().unwrap());

    let batch_file = temp.path().join("tasks.toml");
    fs::write(
        &batch_file,
        r#"
title = "My Batch"

# Profiles are declared so the fixture is valid whether or not the developer's
# own .aid/project.toml sets require_task_profile. This test is about workgroup
# naming, not about profile enforcement.
[defaults]
difficulty = "simple"
budget = "standard"
urgency = "normal"
rigor = "standard"

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
            group: None,
            repo_root: None,
            parallel: false,
            analyze: false,
            wait: false,
            dry_run: true,
            no_prompt: false,
            yes: false,
            force: false,
            max_concurrent: None,
        },
    )
    .await
    .unwrap();

    let workgroups = store.list_workgroups().unwrap();
    assert_eq!(workgroups.len(), 1);
    assert_eq!(workgroups[0].name, "My Batch");
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
# Declared for the same reason as the fixture above: this test is about
# workgroup assignment, not about profile enforcement, so it must not depend
# on the developer's own require_task_profile setting.
[defaults]
difficulty = "simple"
budget = "standard"
urgency = "normal"
rigor = "standard"

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
            repo_root: None,
            parallel: false,
            analyze: false,
            wait: false,
            dry_run: true,
            no_prompt: false,
            yes: false,
            force: false,
            max_concurrent: None,
        },
    )
    .await
    .unwrap();

    let tasks = store.list_tasks(TaskFilter::All).unwrap();
    assert_eq!(tasks.len(), 2);
    assert!(
        tasks
            .iter()
            .all(|task| task.workgroup_id.as_deref() == Some("wg-shared"))
    );
}

#[test]
fn failed_batch_reports_and_archives_before_returning_error() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(temp.path());
    let batch_file = temp.path().join("failed.toml");
    fs::write(
        &batch_file,
        "[[tasks]]\nname = 'failed-task'\nagent = 'codex'\nprompt = 'fail'\n",
    )
    .unwrap();
    let config = batch::parse_batch_file(&batch_file).unwrap();
    let store = Store::open_memory().unwrap();
    seed_task(&store, "t-failed", TaskStatus::Failed, None);
    let dispatch = BatchDispatchResult {
        task_ids: vec!["t-failed".to_string()],
        outcomes: vec![BatchTaskOutcome::Failed],
    };
    let summary = batch_summary(
        &dispatch.outcomes,
        &dispatch.task_ids,
        &config.tasks,
        &store,
        Instant::now(),
        None,
    );
    assert!(summary.contains("1 failed"));
    let error = finalize_batch(
        &batch_file,
        &dispatch,
        &["t-failed".to_string()],
        &config,
        &store,
        Instant::now(),
        None,
        batch_wait_error(WaitOutcome::Failed(vec!["t-failed".to_string()])),
        1,
    )
    .unwrap_err();

    assert!(error.to_string().contains("t-failed"));
    let archives = fs::read_dir(crate::support::paths::aid_dir().join("batches"))
        .unwrap()
        .count();
    assert_eq!(archives, 1);
}
