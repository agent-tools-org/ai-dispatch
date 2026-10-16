// Usage budget and synthetic task regression fixtures.
// Tests collection against the real in-memory Store.
use super::{check_budget_status_for_project, collect_usage};
use crate::config::AidConfig;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;

use crate::cost::usage_report::render_usage;

pub(super) fn make_task(id: &str, agent: AgentKind, tokens: i64, cost_usd: f64) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Done,
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
        tokens: Some(tokens),
        prompt_tokens: None,
        duration_ms: Some(1000),
        requested_model: None,
        observed_model: None,
        attribution_source: None,
        cost_usd: Some(cost_usd),
        exit_code: None,
        created_at: Local::now(),
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

fn make_project_task(id: &str, agent: AgentKind, tokens: i64, cost_usd: f64, repo_path: &str) -> Task {
    let mut task = make_task(id, agent, tokens, cost_usd);
    task.repo_path = Some(repo_path.to_string());
    task.project_id = std::path::Path::new(repo_path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    task
}

#[test]
fn renders_configured_budget_usage() {
    let store = Store::open_memory().unwrap();
    store
        .insert_task(&make_task("t-1", AgentKind::Codex, 12_000, 0.45))
        .unwrap();
    let config: AidConfig = toml::from_str(
        r#"
        [[usage.budget]]
        name = "codex-dev"
        agent = "codex"
        window = "24h"
        task_limit = 5
        token_limit = 50000
        cost_limit_usd = 2.0
        "#,
    )
    .unwrap();

    let snapshot = collect_usage(&store, &config).unwrap();
    let rendered = render_usage(&snapshot);
    assert!(rendered.contains("Tracked Task History"));
    assert!(rendered.contains("Configured Budgets"));
    assert!(rendered.contains("1/5"));
    assert!(rendered.contains("12.0k/50.0k"));
}

#[test]
fn project_budget_cost_cap_counts_matching_project() {
    let store = Store::open_memory().unwrap();
    store
        .insert_task(&make_project_task(
            "t-a",
            AgentKind::Codex,
            1_000,
            1.20,
            "/work/project-a",
        ))
        .unwrap();
    store
        .insert_task(&make_project_task(
            "t-b",
            AgentKind::Gemini,
            1_000,
            0.75,
            "/work/project-b",
        ))
        .unwrap();
    let config: AidConfig = toml::from_str(
        r#"
        [[usage.budget]]
        name = "project-a"
        cost_limit_usd = 1.0
        "#,
    )
    .unwrap();

    let status = check_budget_status_for_project(&store, &config, Some("project-a")).unwrap();
    assert!(status.over_limit);
    assert!(
        status
            .message
            .unwrap()
            .contains("cost limit reached ($1.20/$1.00)")
    );
}

#[test]
fn other_project_budget_does_not_block_dispatch() {
    let store = Store::open_memory().unwrap();
    store
        .insert_task(&make_project_task(
            "t-a",
            AgentKind::Codex,
            1_000,
            25.00,
            "/work/project-a",
        ))
        .unwrap();
    store
        .insert_task(&make_project_task(
            "t-b",
            AgentKind::Gemini,
            1_000,
            1.00,
            "/work/project-b",
        ))
        .unwrap();
    let config: AidConfig = toml::from_str(
        r#"
        [[usage.budget]]
        name = "project-a"
        cost_limit_usd = 20.0

        [[usage.budget]]
        name = "project-b"
        cost_limit_usd = 20.0
        "#,
    )
    .unwrap();

    let status = check_budget_status_for_project(&store, &config, Some("project-b")).unwrap();
    assert!(!status.over_limit);
    assert!(!status.near_limit);
}

#[test]
fn unrelated_large_usage_does_not_trip_small_project_budget() {
    let store = Store::open_memory().unwrap();
    store
        .insert_task(&make_project_task(
            "t-smart-router",
            AgentKind::Codex,
            1_000,
            1.00,
            "/work/smart-router",
        ))
        .unwrap();
    store
        .insert_task(&make_project_task(
            "t-global",
            AgentKind::Gemini,
            1_000,
            30_000.00,
            "/work/unrelated",
        ))
        .unwrap();
    let config: AidConfig = toml::from_str(
        r#"
        [[usage.budget]]
        name = "smart-router"
        cost_limit_usd = 20.0
        "#,
    )
    .unwrap();

    let status = check_budget_status_for_project(&store, &config, Some("smart-router")).unwrap();
    assert!(!status.over_limit);
    assert!(!status.near_limit);
}

#[test]
fn per_agent_budget_ignores_other_agents() {
    let store = Store::open_memory().unwrap();
    store
        .insert_task(&make_task("t-codex", AgentKind::Codex, 500, 0.25))
        .unwrap();
    store
        .insert_task(&make_task("t-gemini", AgentKind::Gemini, 5_000, 5.00))
        .unwrap();
    let config: AidConfig = toml::from_str(
        r#"
        [[usage.budget]]
        name = "codex-only"
        agent = "codex"
        token_limit = 1000
        cost_limit_usd = 1.0
        "#,
    )
    .unwrap();

    let status = check_budget_status_for_project(&store, &config, None).unwrap();
    assert!(!status.over_limit);
    assert!(!status.near_limit);
}
