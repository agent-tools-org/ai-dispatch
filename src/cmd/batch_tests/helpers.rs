// Tests for batch helpers (path/workgroup/summary).
// Exports: (tests only)
// Deps: super::shared + batch_helpers
use crate::background::{BackgroundRunSpec, save_spec};
use crate::batch;
use super::shared::{make_task, seed_task};
use crate::paths::AidHomeGuard;
use crate::store::Store;
use crate::types::{AgentKind, TaskFilter, TaskStatus};
use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

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
        &[make_task("first-task", false, None), make_task("second-task", false, None)],
        &store,
        Instant::now() - Duration::from_secs(42),
        None,
    );

    assert_eq!(summary, "[batch] 2/2 done, 0 failed, 0 skipped. Time: 42s");
}

#[test]
fn failed_batch_reports_and_archives_before_returning_error() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(temp.path());
    let batch_file = temp.path().join("failed.toml");
    fs::write(&batch_file, "[[tasks]]\nname = 'failed-task'\nagent = 'codex'\nprompt = 'fail'\n").unwrap();
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
    let archives = fs::read_dir(crate::paths::aid_dir().join("batches")).unwrap().count();
    assert_eq!(archives, 1);
}
#[test]
fn reconcile_and_poll_completed_tasks_marks_zombies_failed() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();
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
        agent_name: "codex".to_string(),
        prompt: "prompt".to_string(),
        dir: Some(".".to_string()),
        output: None,
        result_file: None,
        model: None,
        verify: None,
        setup: None,
        iterate: None,
        eval: None,
        eval_feedback_template: None,
        judge: None,
        max_duration_mins: None,
        idle_timeout_secs: None,
        retry: 0,
        group: None,
        skills: vec![],
        checklist: vec![],
        hooks: vec![],
        template: None,
        worktree: None,
        base_branch: None,
        peer_review: None,
        audit: false,
        scope: vec![],
        interactive: true,
        on_done: None,
        cascade: vec![],
        parent_task_id: None,
        env: None,
        env_forward: None,
        agent_pid: None,
        sandbox: false,
        read_only: false,
        audit_report_mode: false,
        container: None,
        link_deps: true,
        pre_task_dirty_paths: None,
        detached: false,
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
