// Dispatch evidence coverage: one event per task, optional balances, explicit exhausted routes.
// Uses an isolated aidbar cache and AID_HOME with an in-memory store.

use super::*;
use crate::routing::quota::live::CacheDirGuard;
use crate::support::paths::AidHomeGuard;
use crate::types::{Task, TaskStatus, VerifyStatus};

fn task() -> Task {
    Task {
        id: TaskId("t-billing".into()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".into(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Running,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        project_id: None,
        worktree_path: None,
        effective_dir: None,
        worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        requested_model: None,
        observed_model: None,
        attribution_source: None,
        cost_usd: None,
        exit_code: None,
        created_at: chrono::Local::now(),
        completed_at: None,
        verify: None,
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
    }
}

#[test]
fn dispatch_records_overage_once_and_does_not_block_exhausted() {
    let temp = tempfile::tempdir().expect("temp");
    let _home = AidHomeGuard::set(temp.path());
    let _cache = CacheDirGuard::set(temp.path());
    let store = Store::open_memory().expect("store");
    let task = task();
    store.insert_task(&task).expect("task");
    for (state, expected) in [("exhausted", 0), ("in_plan", 0), ("overage", 1), ("overage", 1)] {
        std::fs::write(
            temp.path().join("codex.json"),
            serde_json::json!({
                "ok":true,"snapshot":{"provider":"codex","windows":[],
                    "fetched_at":"2099-01-01T00:00:00Z","billing":{"state":state,
                        "credits":{"balance":12.25,"unlimited":false}}}
            })
            .to_string(),
        )
        .expect("cache");
        record(&store, &task.id, task.agent).expect("explicit dispatch permitted");
        let events = store.get_events(task.id.as_str()).expect("events");
        assert_eq!(events.len(), expected);
        if let Some(event) = events.first() {
            assert_eq!(event.detail, "billing: overage (credits balance 12.25)");
            assert_eq!(
                event.metadata.as_ref().expect("metadata")["billing_state"],
                "overage"
            );
        }
    }
}

#[test]
fn billing_insert_failure_does_not_abort_dispatch_after_running_transition() {
    let temp = tempfile::tempdir().expect("temp");
    let _home = AidHomeGuard::set(temp.path());
    let _cache = CacheDirGuard::set(temp.path());
    let store = std::sync::Arc::new(Store::open_memory().expect("store"));
    let mut task = task();
    task.status = TaskStatus::Pending;
    store.insert_task(&task).expect("task");
    std::fs::write(temp.path().join("codex.json"), serde_json::json!({"ok":true,
        "snapshot":{"provider":"codex","windows":[],"fetched_at":"2099-01-01T00:00:00Z",
            "billing":{"state":"overage"}}}).to_string()).expect("cache");
    store.db().execute_batch(concat!(
        "CREATE TRIGGER reject_billing BEFORE INSERT ON events ",
        "WHEN NEW.detail LIKE 'billing:%' BEGIN SELECT RAISE(FAIL, 'billing test failure'); END;"
    )).expect("trigger");
    assert!(record(&store, &task.id, task.agent).is_err(), "fault must be exercised");
    let prepared = prepared(task);
    super::super::start_dispatch(&store, &prepared, &[]).expect("billing is advisory");
    let saved = store.get_task(prepared.task_id.as_str()).expect("query").expect("task");
    assert_eq!(saved.status, TaskStatus::Running);
    assert!(store.get_events(prepared.task_id.as_str()).expect("events")
        .iter().all(|event| !event.detail.starts_with("billing:")));
}

fn prepared(task: Task) -> super::super::PreparedDispatch {
    super::super::PreparedDispatch {
        detected_project: None,
        project_root: None,
        agent_kind: task.agent,
        agent_display_name: task.agent.as_str().into(),
        requested_skills: Vec::new(),
        effective_model: None,
        budget_active: false,
        task_id: task.id.clone(),
        task,
        workgroup: None,
        wt_path: None,
        effective_dir: None,
    }
}

struct CodexHomeReset;
impl Drop for CodexHomeReset {
    fn drop(&mut self) {
        crate::agent::codex::cli_config::set_test_codex_home(None);
    }
}

#[test]
fn healthy_plan_cannot_release_provider_hold_or_add_plan_billing_or_advice() {
    let temp = tempfile::tempdir().unwrap();
    let _home = AidHomeGuard::set(temp.path());
    let _cache = CacheDirGuard::set(temp.path());
    std::fs::write(temp.path().join("config.toml"), "model_provider='X'").unwrap();
    crate::agent::codex::cli_config::set_test_codex_home(Some(temp.path().to_path_buf()));
    let _reset = CodexHomeReset;
    let route = Some("codex-provider:X");
    crate::routing::rate_limit::mark_rate_limited(
        &AgentKind::Codex, route, "You have hit your usage limit");
    std::fs::write(
        temp.path().join("codex.json"),
        serde_json::json!({"ok":true,
        "snapshot":{"provider":"codex","fetched_at":"2099-01-01T00:00:00Z",
            "windows":[{"label":"5h","used_percent":1,"resets_at":"2099-01-02T00:00:00Z"}],
            "billing":{"state":"overage"}}})
        .to_string(),
    )
    .unwrap();
    assert_injected_plan_does_not_release_x(route);
    let avail = crate::routing::availability::availability(&AgentKind::Codex, route);
    assert_eq!(
        avail.status,
        crate::routing::availability::RouteStatus::Held
    );
    assert!(avail.probe.is_none());
    let store = Store::open_memory().unwrap();
    let mut task = task();
    task.custom_agent_name = route.map(str::to_string);
    store.insert_task(&task).unwrap();
    record(&store, &task.id, task.agent).unwrap();
    assert!(store.get_events(task.id.as_str()).unwrap().is_empty());
    assert_provider_advice_held();
}

fn assert_provider_advice_held() {
    use crate::types::{DeclaredTaskProfile, TaskBudget, TaskDifficulty, TaskRigor, TaskUrgency};
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Codex]);
    let report = crate::agent::selection::advise(
        "Explain routing",
        DeclaredTaskProfile {
            difficulty: TaskDifficulty::Moderate,
            budget: TaskBudget::Standard,
            urgency: TaskUrgency::Normal,
            rigor: TaskRigor::Standard,
        },
        None,
        None,
        None,
        0,
        None,
    );
    let codex = report
        .candidates
        .iter()
        .find(|candidate| candidate.agent == "codex")
        .unwrap();
    assert_eq!(codex.quota.status, "held");
    assert!(codex.exclusion_codes.iter().any(|code| code == "needs_human"));
    assert_eq!(codex.breakdown.billing, 0.0);
    assert!(codex.billing.is_none());
}

fn assert_injected_plan_does_not_release_x(route: Option<&str>) {
    use crate::routing::availability::{decide, RouteStatus};
    let probe = live::snapshot(&AgentKind::Codex).unwrap();
    let marker = crate::routing::rate_limit::marker_path(&AgentKind::Codex, route);
    let content = std::fs::read_to_string(&marker).unwrap();
    let mtime = std::fs::metadata(marker).unwrap().modified().unwrap();
    let provider = decide(&AgentKind::Codex, route, None,
        Some(&content), Some(mtime), Some(probe.clone()));
    assert_eq!(provider.status, RouteStatus::Held);
    assert!(provider.probe.is_none());
    let message = crate::routing::rate_limit::marker_field(&content, "message: ").unwrap();
    crate::routing::rate_limit::mark_rate_limited(&AgentKind::Codex, None, &message);
    let default_marker = crate::routing::rate_limit::marker_path(&AgentKind::Codex, None);
    let default_content = std::fs::read_to_string(&default_marker).unwrap();
    let default_mtime = std::fs::metadata(default_marker).unwrap().modified().unwrap();
    let default = decide(&AgentKind::Codex, None, None,
        Some(&default_content), Some(default_mtime), Some(probe));
    assert_eq!(default.status, RouteStatus::Dispatchable);
}
