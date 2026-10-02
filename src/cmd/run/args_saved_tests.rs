// Worker configuration rebuilt from saved dispatch args across the job-file boundary.
// Every saved RunArgs field must reach the worker; env values are never persisted.
use super::RunArgs;
use crate::agent::classifier::TaskCategory;
use crate::agent::model_validation::ModelSource;
use crate::background::{load_spec_if_exists, save_spec, BackgroundRunSpec};
use crate::store::Store;
use crate::timeout_policy::TimeoutPolicy;
use crate::types::*;
use chrono::Local;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

#[test]
fn worker_rebuild_keeps_every_saved_field() {
    let home = tempfile::tempdir().unwrap();
    let _guard = crate::paths::AidHomeGuard::set(home.path());
    crate::paths::ensure_dirs().unwrap();
    let store = Store::open(&crate::paths::db_path()).unwrap();
    let id = "t-every-field";
    let saved = every_field_set();
    assert_every_field_differs_from_default(&saved);
    let mut row = task(id);
    row.agent = AgentKind::Custom;
    row.custom_agent_name = Some("resolved-agent".into());
    row.effective_dir = Some("/tmp/row-effective".into());
    row.resolved_prompt = Some("resolved prompt".into());
    store.insert_task(&row).unwrap();
    let json = saved.dispatch_args_json().unwrap();
    assert!(!json.contains("secret-value"), "env values must never be persisted");
    store.update_task_dispatch_args(id, &json).unwrap();
    // The detached worker receives only the task id: config comes from the
    // store, runtime facts from the job file.
    save_spec(&spec(id)).unwrap();
    let loaded = load_spec_if_exists(id).unwrap().unwrap();
    let reopened = Store::open(&crate::paths::db_path()).unwrap();
    let worker = RunArgs::for_worker(&reopened, &loaded).unwrap();

    let mut expected = serde_json::to_value(&saved).unwrap();
    for (key, value) in [
        ("agent_name", json!("resolved-agent")),
        ("dir", json!("/tmp/spec")),
        ("prompt", json!("resolved prompt")),
        ("budget", json!(false)),
        ("on_done", Value::Null),
        ("env", json!({"RUNTIME_ONLY": "from-spec"})),
        ("existing_task_id", Value::Null),
        ("foreground", json!(false)),
        ("announce", json!(false)),
        ("background", json!(true)),
    ] {
        expected[key] = value;
    }
    let actual = serde_json::to_value(&worker).unwrap();
    for (key, want) in expected.as_object().unwrap() {
        assert_eq!(&actual[key], want, "field `{key}` changed across the worker rebuild");
    }
}

#[test]
fn dispatch_saves_the_callers_budget_request_not_the_resolved_mode() {
    let home = tempfile::tempdir().unwrap();
    let _guard = crate::paths::AidHomeGuard::set(home.path());
    crate::paths::ensure_dirs().unwrap();
    std::fs::write(crate::paths::config_path(), "[selection]\nbudget_mode = true\n").unwrap();
    std::fs::create_dir_all(crate::paths::aid_dir().join("agents")).unwrap();
    std::fs::write(crate::paths::aid_dir().join("agents/budget-test.toml"),
        "[agent]\nid = \"budget-test\"\ndisplay_name = \"Budget Test\"\ncommand = \"/bin/sh\"\nprompt_mode = \"arg\"\ninteractive_input = false\n").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let mut args = RunArgs {
        agent_name: "budget-test".into(), prompt: "Assess the project".into(),
        dir: Some(dir.path().display().to_string()), read_only: true, ..Default::default()
    };
    let prepared = super::super::run_dispatch_prepare::prepare_dispatch_with(&store, &mut args, |_| true).unwrap();
    let id = prepared.task_id.as_str();
    assert!(prepared.budget_active, "dispatch resolved budget mode for this run");
    // Retries read task.budget and the saved args: neither may carry a mode the caller never asked for.
    assert!(!store.get_task(id).unwrap().unwrap().budget);
    assert!(!RunArgs::saved_for_task(&store, id).unwrap().unwrap().budget);
}

#[tokio::test]
async fn worker_without_a_dispatched_dir_skips_verify() {
    let home = tempfile::tempdir().unwrap();
    let _guard = crate::paths::AidHomeGuard::set(home.path());
    crate::paths::ensure_dirs().unwrap();
    std::fs::create_dir_all(crate::paths::aid_dir().join("agents")).unwrap();
    std::fs::write(crate::paths::aid_dir().join("agents/noop-test.toml"),
        "[agent]\nid = \"noop-test\"\ndisplay_name = \"Noop Test\"\ncommand = \"/bin/sh\"\nprompt_mode = \"arg\"\nfixed_args = [\"-c\", \"true\"]\ninteractive_input = false\n").unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let id = "t-no-dir";
    let marker = home.path().join("verify-ran");
    let verify = format!("touch '{}'", marker.display());
    let mut row = task(id);
    row.agent = AgentKind::Custom;
    row.custom_agent_name = Some("noop-test".into());
    // Dispatch records the caller's cwd on the row even when no dir was given.
    row.effective_dir = Some(home.path().display().to_string());
    row.verify = Some(verify.clone());
    row.verify_status = VerifyStatus::Pending;
    store.insert_task(&row).unwrap();
    let saved = RunArgs { agent_name: "noop-test".into(), verify: Some(verify), ..Default::default() };
    store.update_task_dispatch_args(id, &saved.dispatch_args_json().unwrap()).unwrap();
    let mut job = spec(id);
    job.dir = None;
    save_spec(&job).unwrap();
    assert_eq!(RunArgs::for_worker(&store, &job).unwrap().dir, None);

    crate::background::run_task(store.clone(), id).await.unwrap();

    let done = store.get_task(id).unwrap().unwrap();
    assert_eq!(done.status, TaskStatus::Done);
    assert!(!marker.exists(), "verify must not run without a dispatched working directory");
}

fn every_field_set() -> RunArgs {
    RunArgs {
        agent_name: "saved-agent".into(), prompt: "requested prompt".into(),
        prompt_file: Some("prompt.md".into()), repo: Some("/tmp/repo".into()),
        repo_root: Some("/tmp/root".into()), dir: Some("/tmp/requested".into()),
        output: Some("out.md".into()), result_file: Some("result.md".into()),
        result_file_required: Some(true), model: Some("effective-model".into()),
        model_source: ModelSource::AidResolved,
        declared_difficulty: Some(TaskDifficulty::Complex), declared_budget: Some(TaskBudget::Premium),
        declared_urgency: Some(TaskUrgency::Urgent), declared_rigor: Some(TaskRigor::Critical),
        declared_egress: TaskEgress::Local, kind: Some(TaskCategory::ComplexImpl),
        worktree: Some("feat/x".into()), base_branch: Some("develop".into()), group: Some("wg-1".into()),
        verify: Some("true".into()), setup: Some("make setup".into()), iterate: Some(3),
        eval: Some("make eval".into()), eval_feedback_template: Some("fix {output}".into()),
        judge: Some("gemini".into()), peer_review: Some("claude".into()),
        max_duration_mins: Some(42), max_task_cost: Some(1.5), retry: 2,
        context: vec!["ctx.md".into()], checklist: vec!["check".into()], skills: vec!["implementer".into()],
        hooks: vec!["on_fail:true".into()], template: Some("bug-fix".into()),
        background: true, dry_run: true, announce: true, foreground: true,
        parent_task_id: Some("t-parent".into()), on_done: Some("echo done".into()),
        cascade: vec!["opencode".into()], read_only: true, audit_report_mode: true, sandbox: true,
        container: Some("ubuntu:latest".into()), remote_build: Some("chosen-box".into()),
        backup: Some("gdrive:x".into()), no_backup: true, budget: true, best_of: Some(2),
        metric: Some("wc -l".into()), session_id: Some("session-1".into()), team: Some("dev".into()),
        context_from: vec!["t-ctx".into()],
        batch_siblings: vec![("a".into(), "b".into(), "c".into())], scope: vec!["src/".into()],
        env: Some(HashMap::from([("TOKEN".into(), "secret-value".into())])),
        env_forward: Some(vec!["FORWARDED".into()]), judge_retry: true,
        existing_task_id: Some(TaskId("t-existing".into())), timeout: Some(71),
        idle_timeout_secs: Some(7),
        timeout_policy: TimeoutPolicy {
            idle: Duration::from_secs(7),
            ..TimeoutPolicy::default()
        },
        audit: true,
        audit_explicit: true,
        no_audit: true,
        suppress_nested_repo_warning: true,
        link_deps: false,
        force_default_model: true,
    }
}

fn assert_every_field_differs_from_default(saved: &RunArgs) {
    let default = serde_json::to_value(RunArgs::default()).unwrap();
    let saved = serde_json::to_value(saved).unwrap();
    for (key, value) in saved.as_object().unwrap() {
        assert_ne!(value, &default[key], "fixture leaves `{key}` at its default");
    }
}

fn spec(task_id: &str) -> BackgroundRunSpec {
    serde_json::from_value(json!({
        "task_id": task_id, "worker_pid": null, "agent_name": "spec-agent",
        "prompt": "spec prompt", "dir": "/tmp/spec", "output": null, "model": "spec-model",
        "budget": false, "verify": null, "retry": 0, "group": null, "interactive": true,
        "env": {"RUNTIME_ONLY": "from-spec"}, "foreground": false
    }))
    .unwrap()
}

fn task(task_id: &str) -> Task {
    Task {
        id: TaskId(task_id.to_string()), agent: AgentKind::Codex, custom_agent_name: None,
        prompt: "prompt".to_string(), resolved_prompt: None, category: None,
        status: TaskStatus::Running, parent_task_id: None, workgroup_id: None,
        caller_kind: None, caller_session_id: None, agent_session_id: None,
        repo_path: None, project_id: None, worktree_path: None, effective_dir: None,
        worktree_branch: None, final_head_sha: None, final_branch: None, start_sha: None,
        log_path: None, output_path: None, tokens: None, prompt_tokens: None, duration_ms: None,
        requested_model: None, observed_model: None, attribution_source: None,
        cost_usd: None, exit_code: None, created_at: Local::now(), completed_at: None,
        verify: None, verify_status: VerifyStatus::Skipped, pending_reason: None,
        read_only: false, budget: false, audit_verdict: None, audit_report_path: None,
        delivery_assessment: None,
    }
}
