// Audit integration tests for `aid run`.
// Exports: test-only coverage for post-DONE AIC execution and config wiring.
// Deps: super, crate::{batch, project, store, types}, tempfile.

use super::{RunArgs, maybe_run_post_done_audit};
use crate::batch::parse_batch_file;
use crate::project::{ProjectAuditConfig, ProjectConfig};
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    crate::aic::test_env_lock()
}

fn done_task(task_id: &str) -> Task {
    Task {
        id: TaskId(task_id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "audit task".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        worktree_path: None,
        worktree_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: Some(1_000),
        model: None,
        cost_usd: None,
        exit_code: Some(0),
        created_at: Local::now(),
        completed_at: Some(Local::now()),
        verify: None,
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
    }
}

fn install_aic_shim(dir: &Path, body: &str) {
    let path = dir.join("aic");
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).unwrap();
}

fn set_env(key: &str, value: impl AsRef<std::ffi::OsStr>) {
    unsafe { env::set_var(key, value) }
}

fn remove_env(key: &str) {
    unsafe { env::remove_var(key) }
}

fn audit_args(enabled: bool) -> RunArgs {
    RunArgs {
        agent_name: "codex".to_string(),
        prompt: "audit".to_string(),
        audit: enabled,
        audit_explicit: enabled,
        ..Default::default()
    }
}

fn run_audit_for_task(store: &Store, task_id: &str, args: &RunArgs) {
    maybe_run_post_done_audit(store, &TaskId(task_id.to_string()), args, None, None).unwrap();
}

#[test]
fn audit_skipped_when_aic_not_found() {
    let _guard = env_lock();
    set_env("AIC_TEST_PRESENT", "0");
    let store = Store::open_memory().unwrap();
    store.insert_task(&done_task("t-audit-skip")).unwrap();

    run_audit_for_task(&store, "t-audit-skip", &audit_args(true));

    let task = store.get_task("t-audit-skip").unwrap().unwrap();
    assert_eq!(task.audit_verdict.as_deref(), Some("skipped"));
    assert_eq!(task.audit_report_path, None);
    let events = store.get_events("t-audit-skip").unwrap();
    assert!(events.iter().any(|event| event.detail == "audit skipped: aic binary not found"));
    remove_env("AIC_TEST_PRESENT");
}

#[test]
fn audit_records_pass_verdict() {
    let _guard = env_lock();
    let temp = tempfile::tempdir().unwrap();
    install_aic_shim(
        temp.path(),
        "if [ \"$1\" = \"--version\" ]; then exit 0; fi\nprintf 'report: /tmp/foo.md\\n'\nexit 0",
    );
    set_env("AIC_TEST_BINARY", temp.path().join("aic"));
    let store = Store::open_memory().unwrap();
    store.insert_task(&done_task("t-audit-pass")).unwrap();

    run_audit_for_task(&store, "t-audit-pass", &audit_args(true));

    let task = store.get_task("t-audit-pass").unwrap().unwrap();
    assert_eq!(task.audit_verdict.as_deref(), Some("pass"));
    assert_eq!(task.audit_report_path.as_deref(), Some("/tmp/foo.md"));
    remove_env("AIC_TEST_BINARY");
}

#[test]
fn audit_records_fail_verdict() {
    let _guard = env_lock();
    let temp = tempfile::tempdir().unwrap();
    install_aic_shim(
        temp.path(),
        "if [ \"$1\" = \"--version\" ]; then exit 0; fi\nprintf 'report: /tmp/fail.md\\n'\nexit 1",
    );
    set_env("AIC_TEST_BINARY", temp.path().join("aic"));
    let store = Store::open_memory().unwrap();
    store.insert_task(&done_task("t-audit-fail")).unwrap();

    run_audit_for_task(&store, "t-audit-fail", &audit_args(true));

    let task = store.get_task("t-audit-fail").unwrap().unwrap();
    assert_eq!(task.audit_verdict.as_deref(), Some("fail"));
    remove_env("AIC_TEST_BINARY");
}

#[test]
fn audit_records_error_verdict() {
    let _guard = env_lock();
    let temp = tempfile::tempdir().unwrap();
    install_aic_shim(
        temp.path(),
        "if [ \"$1\" = \"--version\" ]; then exit 0; fi\nexit 200",
    );
    set_env("AIC_TEST_BINARY", temp.path().join("aic"));
    let store = Store::open_memory().unwrap();
    store.insert_task(&done_task("t-audit-error")).unwrap();

    run_audit_for_task(&store, "t-audit-error", &audit_args(true));

    let task = store.get_task("t-audit-error").unwrap().unwrap();
    assert_eq!(task.audit_verdict.as_deref(), Some("error"));
    remove_env("AIC_TEST_BINARY");
}

#[test]
fn audit_respects_timeout() {
    let _guard = env_lock();
    let temp = tempfile::tempdir().unwrap();
    install_aic_shim(
        temp.path(),
        "if [ \"$1\" = \"--version\" ]; then exit 0; fi\nsleep 2\nprintf 'report: /tmp/late.md\\n'\nexit 0",
    );
    set_env("AIC_TEST_BINARY", temp.path().join("aic"));
    set_env("AID_AUDIT_TIMEOUT_SECS", "1");
    let store = Store::open_memory().unwrap();
    store.insert_task(&done_task("t-audit-timeout")).unwrap();

    run_audit_for_task(&store, "t-audit-timeout", &audit_args(true));

    let task = store.get_task("t-audit-timeout").unwrap().unwrap();
    assert_eq!(task.audit_verdict.as_deref(), Some("error"));
    remove_env("AID_AUDIT_TIMEOUT_SECS");
    remove_env("AIC_TEST_BINARY");
}

#[test]
fn project_audit_auto_triggers_without_cli_flag() {
    let _guard = env_lock();
    let temp = tempfile::tempdir().unwrap();
    install_aic_shim(
        temp.path(),
        "if [ \"$1\" = \"--version\" ]; then exit 0; fi\nprintf 'report: /tmp/project.md\\n'\nexit 0",
    );
    set_env("AIC_TEST_BINARY", temp.path().join("aic"));
    let store = Store::open_memory().unwrap();
    store.insert_task(&done_task("t-project-audit")).unwrap();
    let mut args = RunArgs::default();
    let project = ProjectConfig { id: "demo".to_string(), audit: ProjectAuditConfig { auto: true }, ..Default::default() };

    super::run_dispatch_resolve::apply_project_defaults(&mut args, Some(&project));
    run_audit_for_task(&store, "t-project-audit", &args);

    let task = store.get_task("t-project-audit").unwrap().unwrap();
    assert_eq!(task.audit_verdict.as_deref(), Some("pass"));
    remove_env("AIC_TEST_BINARY");
}

#[test]
fn no_audit_overrides_project_audit_auto() {
    let _guard = env_lock();
    let temp = tempfile::tempdir().unwrap();
    install_aic_shim(
        temp.path(),
        "if [ \"$1\" = \"--version\" ]; then exit 0; fi\nprintf 'report: /tmp/no-audit.md\\n'\nexit 0",
    );
    set_env("AIC_TEST_BINARY", temp.path().join("aic"));
    let store = Store::open_memory().unwrap();
    store.insert_task(&done_task("t-no-audit")).unwrap();
    let mut args = RunArgs { no_audit: true, ..Default::default() };
    let project = ProjectConfig {
        id: "demo".to_string(),
        audit: ProjectAuditConfig { auto: true },
        ..Default::default()
    };

    super::run_dispatch_resolve::apply_project_defaults(&mut args, Some(&project));
    run_audit_for_task(&store, "t-no-audit", &args);

    let task = store.get_task("t-no-audit").unwrap().unwrap();
    assert_eq!(task.audit_verdict, None);
    remove_env("AIC_TEST_BINARY");
}

#[test]
fn batch_task_level_audit_override_wins() {
    let _guard = env_lock();
    let temp = tempfile::tempdir().unwrap();
    install_aic_shim(
        temp.path(),
        "if [ \"$1\" = \"--version\" ]; then exit 0; fi\nprintf 'report: /tmp/batch.md\\n'\nexit 0",
    );
    set_env("AIC_TEST_BINARY", temp.path().join("aic"));
    let batch_file = temp.path().join("tasks.toml");
    fs::write(&batch_file, "[defaults]\nagent = \"codex\"\naudit = false\n[[tasks]]\nname = \"plain\"\nprompt = \"plain\"\n[[tasks]]\nname = \"audited\"\nprompt = \"audited\"\naudit = true\n").unwrap();
    let config = parse_batch_file(&batch_file).unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let plain_args = RunArgs {
        agent_name: config.tasks[0].agent.clone(),
        prompt: config.tasks[0].prompt.clone(),
        audit: config.tasks[0].audit.unwrap_or(false),
        audit_explicit: config.tasks[0].audit.is_some(),
        ..Default::default()
    };
    let audited_args = RunArgs {
        agent_name: config.tasks[1].agent.clone(),
        prompt: config.tasks[1].prompt.clone(),
        audit: config.tasks[1].audit.unwrap_or(false),
        audit_explicit: config.tasks[1].audit.is_some(),
        ..Default::default()
    };

    assert!(!plain_args.audit);
    assert!(audited_args.audit);
    assert!(audited_args.audit_explicit);
    store.insert_task(&done_task("t-batch-audit")).unwrap();
    run_audit_for_task(store.as_ref(), "t-batch-audit", &audited_args);

    let task = store.get_task("t-batch-audit").unwrap().unwrap();
    assert_eq!(task.audit_verdict.as_deref(), Some("pass"));
    remove_env("AIC_TEST_BINARY");
}

#[test]
fn show_header_includes_audit_verdict_when_present() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = done_task("t-show-audit");
    task.audit_verdict = Some("pass".to_string());
    task.audit_report_path = Some("/tmp/report.md".to_string());
    store.insert_task(&task).unwrap();

    let summary = crate::cmd::show::summary_text(&store, task.id.as_str()).unwrap();

    assert!(summary.contains("Audit: pass (report: /tmp/report.md)"));
}
