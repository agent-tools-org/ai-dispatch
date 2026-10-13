// Capacity continuation session, retry-budget and fake-agent lifecycle regressions.
// Deps: lifecycle, in-memory Store, isolated aid home and custom shell agent.

use super::*;
use crate::paths;

const CAPACITY: &str = "Selected model is at capacity. Please try a different model.";

fn task(id: &str, dir: &std::path::Path) -> Task {
    Task {
        id: TaskId(id.into()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "Original task".into(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Failed,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: Some("capacity-session".into()),
        repo_path: None,
        project_id: None,
        worktree_path: None,
        effective_dir: Some(dir.display().to_string()),
        worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        requested_model: Some("gpt-6.1-sol".into()),
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
fn provider_capacity_continuation_reuses_codex_session_without_consuming_retry() {
    let dir = tempfile::tempdir().expect("dir");
    let store = Store::open_memory().expect("store");
    let row = task("t-capacity-args", dir.path());
    store.insert_task(&row).expect("task");
    let args = RunArgs {
        retry: 3,
        dir: Some(dir.path().display().to_string()),
        ..Default::default()
    };
    let child = continuation_args(&store, &args, &row).expect("continuation");
    assert_eq!(child.session_id.as_deref(), Some("capacity-session"));
    assert_eq!(child.retry, 3);
    assert_eq!(child.model, row.requested_model);
    assert_eq!(child.parent_task_id.as_deref(), Some(row.id.as_str()));
    let mut read_only = row;
    read_only.read_only = true;
    let readonly = continuation_args(&store, &args, &read_only).expect("readonly");
    assert!(readonly.read_only);
    assert!(readonly.prompt.contains("read-only"));
    assert!(!readonly.prompt.contains("commit"));
}

fn install_agent(home: &std::path::Path, name: &str, command: &str) {
    std::fs::create_dir_all(home.join("agents")).expect("agents");
    let args = serde_json::to_string(&vec!["-c", command]).expect("args");
    std::fs::write(
        home.join(format!("agents/{name}.toml")),
        format!(
            "[agent]\nid = '{name}'\ndisplay_name = '{name}'\ncommand = '/bin/sh'\n\
         prompt_mode = 'arg'\nfixed_args = {args}\ninteractive_input = false\n"
        ),
    )
    .expect("agent");
}

#[tokio::test]
async fn provider_capacity_repeated_refusal_names_hold_expiry_when_no_route_is_launchable() {
    let home = tempfile::tempdir().expect("home");
    let _home = paths::AidHomeGuard::set(home.path());
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![]);
    paths::ensure_dirs().expect("dirs");
    let store = Arc::new(Store::open_memory().expect("store"));
    let row = task("t-capacity-no-route", home.path());
    store.insert_task(&row).expect("task");
    let error = serde_json::json!({"type":"error", "message":CAPACITY});
    std::fs::write(paths::log_path(row.id.as_str()), error.to_string()).expect("refusal");
    rate_limit::mark_rate_limited_for_model(
        &row.agent,
        None,
        row.requested_model.as_deref(),
        CAPACITY,
    );
    record(
        &store,
        &row.id,
        "provider capacity: continuation already used".into(),
        true,
    )
    .expect("marker");
    let args = RunArgs {
        agent_name: "codex".into(),
        dir: row.effective_dir.clone(),
        ..Default::default()
    };
    assert!(
        continue_capacity(&store, &args, &row.id)
            .await
            .expect("capacity")
            .is_none()
    );
    let detail = store.latest_error(row.id.as_str()).expect("failure");
    assert!(detail.contains("provider capacity held until"));
    assert!(detail.contains("continuation already used"));
    assert!(detail.contains("gpt-6.1-sol"));
    let events = store.get_events(row.id.as_str()).expect("events");
    let rendered = crate::board::render_task_detail(&row, &events, None);
    assert!(rendered.contains("Status: provider capacity"));
}

#[path = "capacity_continuation_flow_tests.rs"]
mod flow;

#[path = "capacity_cancellation_tests.rs"]
mod cancellation;

#[path = "capacity_round2_tests.rs"]
mod round2;

#[test]
fn provider_capacity_regression_never_selects_automatic_advice_fallback() {
    let home = tempfile::tempdir().expect("home");
    let _home = paths::AidHomeGuard::set(home.path());
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Codex, AgentKind::Gemini]);
    let store = Store::open_memory().expect("store");
    let row = task("t-capacity-advice", home.path());
    store.insert_task(&row).expect("task");
    let args = RunArgs {
        agent_name: "codex".into(),
        ..Default::default()
    };
    assert!(cascade_args(&store, &args, &row).expect("route").is_none());
}
