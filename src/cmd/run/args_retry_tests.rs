// Retry configuration rebuilt from saved dispatch args or legacy task rows.
// Covers every persisted field, model provenance, and session eligibility.
use super::RunArgs;
use crate::agent::{classifier::TaskCategory, model_validation::ModelSource};
use crate::{store::Store, timeout_policy::TimeoutPolicy, types::*};
use chrono::Local;
use serde_json::{json, Value};
use std::{collections::HashMap, time::Duration};

#[test]
fn retry_fixture_sets_every_field() {
    assert_every_field_differs_from_default(&every_field_set());
}

#[test]
fn retry_keeps_every_saved_field_except_resolved_task_facts() {
    let store = Store::open_memory().unwrap();
    let saved = every_field_set();
    let mut row = task("t-retry-every-field");
    row.agent = AgentKind::Custom;
    row.custom_agent_name = Some("row-agent".into());
    row.repo_path = Some("/row/repo".into());
    row.output_path = Some("row-output".into());
    row.requested_model = Some("row-model".into());
    row.workgroup_id = Some("wg-row".into());
    row.verify = Some("row-verify".into());
    row.agent_session_id = Some("row-session".into());
    store.insert_task(&row).unwrap();
    store.update_task_dispatch_args(row.id.as_str(), &saved.dispatch_args_json().unwrap()).unwrap();
    let actual = serde_json::to_value(RunArgs::for_retry(&store, &row).unwrap()).unwrap();
    let mut expected = serde_json::to_value(&saved).unwrap();
    for (key, value) in [
        ("agent_name", json!("row-agent")), ("model", json!("row-model")),
        ("session_id", Value::Null), ("existing_task_id", Value::Null),
        ("env", Value::Null), ("repo", json!("/tmp/repo")),
    ] {
        expected[key] = value;
    }
    assert_eq!(actual, expected);
    let mut no_repo = saved.clone();
    no_repo.repo = None;
    store.update_task_dispatch_args(row.id.as_str(), &no_repo.dispatch_args_json().unwrap()).unwrap();
    assert_eq!(RunArgs::for_retry(&store, &row).unwrap().repo, row.repo_path);
}

#[test]
fn retry_without_saved_args_uses_legacy_row() {
    let store = Store::open_memory().unwrap();
    let mut row = task("t-legacy-retry");
    row.repo_path = Some("/legacy/repo".into());
    row.output_path = Some("legacy-output".into());
    row.requested_model = Some("legacy-model".into());
    row.workgroup_id = Some("wg-legacy".into());
    row.verify = Some("legacy-verify".into());
    row.read_only = true;
    row.budget = true;
    row.agent_session_id = Some("legacy-session".into());
    store.insert_task(&row).unwrap();
    let expected = RunArgs {
        agent_name: "codex".into(), repo: row.repo_path.clone(), dir: row.repo_path.clone(),
        output: row.output_path.clone(), model: row.requested_model.clone(),
        model_source: ModelSource::AidResolved, group: row.workgroup_id.clone(),
        verify: row.verify.clone(), read_only: true, budget: true,
        session_id: row.agent_session_id.clone(), ..Default::default()
    };
    assert_eq!(serde_json::to_value(RunArgs::for_retry(&store, &row).unwrap()).unwrap(),
        serde_json::to_value(expected).unwrap());
}

#[test]
fn retry_keeps_user_model_and_only_the_current_resumable_session() {
    let store = Store::open_memory().unwrap();
    let mut row = task("t-retry-provenance");
    let mut saved = every_field_set();
    saved.model_source = ModelSource::UserSupplied;
    row.requested_model = Some("resolved-other".into());
    store.insert_task(&row).unwrap();
    store.update_task_dispatch_args(row.id.as_str(), &saved.dispatch_args_json().unwrap()).unwrap();
    assert_eq!(RunArgs::for_retry(&store, &row).unwrap().model, saved.model);
    assert!(RunArgs::for_retry(&store, &row).unwrap().session_id.is_none());
    row.agent_session_id = Some("current-session".into());
    assert_eq!(RunArgs::for_retry(&store, &row).unwrap().session_id, row.agent_session_id);
    row.agent = AgentKind::Custom;
    assert!(RunArgs::for_retry(&store, &row).unwrap().session_id.is_none());
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

#[test]
fn advised_model_and_unknown_default_survive_saved_retry_and_source_reset() {
    let store = Store::open_memory().expect("store");
    let mut row = task("t-advised-retry");
    row.requested_model = Some("different-row-model".into());
    store.insert_task(&row).expect("task");
    for model in [Some("exact-model".to_string()), None] {
        let saved = RunArgs { agent_name: "codex".into(), model: model.clone(),
            model_source: ModelSource::Advised, force_default_model: model.is_none(),
            ..Default::default() };
        store.update_task_dispatch_args(row.id.as_str(), &saved.dispatch_args_json().expect("serialize")).expect("save");
        let mut retry = RunArgs::for_retry(&store, &row).expect("retry");
        assert_eq!(retry.model, model);
        assert_eq!(retry.model_source, ModelSource::Advised);
        assert_eq!(retry.force_default_model, model.is_none());
        crate::cmd::run::switch_agent(&mut retry, "codex".into());
        assert_eq!(retry.model_source, ModelSource::Advised);
        crate::cmd::run::switch_agent(&mut retry, "agy".into());
        assert_eq!(retry.model_source, ModelSource::AidResolved);
        assert!(retry.model.is_none() && !retry.force_default_model);
    }
}
