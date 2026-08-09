// Unit tests for aid stats aggregation and rendering.
// Deps: super (cmd::stats)

use super::*;
use chrono::Duration;
use crate::types::{AttributionSource, EventKind, TaskEvent, TaskId, TaskStatus, VerifyStatus};

fn task(id: &str, agent: AgentKind, status: TaskStatus, age_days: i64, model: &str, cost_usd: Option<f64>, duration_ms: Option<i64>, tokens: i64) -> Task {
    Task { id: TaskId(id.to_string()), agent, custom_agent_name: None, prompt: "prompt".to_string(), resolved_prompt: None, category: None, status, parent_task_id: None, workgroup_id: None, caller_kind: None, caller_session_id: None, agent_session_id: None, repo_path: None, worktree_path: None, worktree_branch: None, final_head_sha: None, final_branch: None, start_sha: None, log_path: None, output_path: None, tokens: Some(tokens), prompt_tokens: None, duration_ms, requested_model: Some(model.to_string()), observed_model: Some(model.to_string()), attribution_source: Some(AttributionSource::Echoed), cost_usd, exit_code: None, created_at: Local::now() - Duration::days(age_days), completed_at: None, verify: None, verify_status: VerifyStatus::Skipped, pending_reason: None, read_only: false, budget: false, audit_verdict: None, audit_report_path: None, delivery_assessment: None }
}

#[test]
fn collects_agent_failure_and_model_stats() {
    let store = Store::open_memory().unwrap();
    let now = Local::now();
    for task in [task("t-1", AgentKind::Codex, TaskStatus::Done, 1, "gpt-5.4", Some(10.0), Some(120_000), 1_000), task("t-2", AgentKind::Codex, TaskStatus::Failed, 2, "gpt-5.4", Some(5.0), Some(60_000), 1_000), task("t-3", AgentKind::Cursor, TaskStatus::Merged, 1, "composer-2", None, Some(90_000), 1_000), task("t-4", AgentKind::OpenCode, TaskStatus::Failed, 8, "glm-4.7", Some(1.0), Some(30_000), 1_000)] {
        store.insert_task(&task).unwrap();
    }
    store.insert_event(&TaskEvent { task_id: TaskId("t-2".to_string()), timestamp: now, event_kind: EventKind::Error, detail: "verify failed (cargo check)".to_string(), metadata: None }).unwrap();
    let stats = collect(&store, UsageWindow::Days(7), None, now).unwrap();
    assert_eq!(stats.agent_rows[0], AgentRow { agent: "codex".to_string(), tasks: 2, share_pct: 66, success_rate: 50.0, avg_duration_ms: Some(90_000), cost: "$15.00".to_string() });
    assert_eq!(stats.agent_rows[1], AgentRow { agent: "cursor".to_string(), tasks: 1, share_pct: 33, success_rate: 100.0, avg_duration_ms: Some(90_000), cost: "subscription".to_string() });
    assert_eq!(stats.failure_rows, vec![FailureRow { label: "verify failed".to_string(), tasks: 1, agents: vec![("codex".to_string(), 1)] }]);
    assert_eq!(stats.model_rows[0], ModelRow { model: "gpt-5.4".to_string(), tasks: 2, cost: "$15.00".to_string() });
    assert_eq!(stats.activity_by_day.len(), 7);
}

#[test]
fn stats_does_not_panic_on_zero_duration_count() {
    let store = Store::open_memory().unwrap();
    let task = task("t-no-dur", AgentKind::Codex, TaskStatus::Done, 1, "gpt-5.4", Some(1.0), None, 1_000);
    store.insert_task(&task).unwrap();

    let stats = collect(&store, UsageWindow::Days(7), None, Local::now()).unwrap();

    assert_eq!(stats.agent_rows[0].avg_duration_ms, None);
}

#[test]
fn timed_out_verification_is_not_counted_as_success() {
    let store = Store::open_memory().unwrap();
    let mut task = task("t-timeout", AgentKind::Codex, TaskStatus::Done, 1, "gpt-5.4", Some(1.0), Some(1_000), 1_000);
    task.verify = Some("cargo test".to_string());
    task.verify_status = VerifyStatus::TimedOut;
    store.insert_task(&task).unwrap();

    let stats = collect(&store, UsageWindow::Days(7), None, Local::now()).unwrap();

    assert_eq!(stats.agent_rows[0].success_rate, 0.0);
}

#[test]
fn render_output_shows_friendly_message_when_no_tasks_match() {
    let stats = StatsSnapshot { agent_rows: Vec::new(), failure_rows: Vec::new(), model_rows: Vec::new(), declared_rows: Vec::new(), activity_by_day: Vec::new(), activity_by_hour: Vec::new(), top_sessions: Vec::new(), total_cost: None, total_tokens: 0, total_tasks: 0 };

    assert_eq!(render_output(&stats, UsageWindow::Days(7), false, false), "No tasks matched the selected filters for last 7 days.\n");
}

#[test]
fn copilot_defaults_to_subscription_cost_when_cost_is_missing() {
    let store = Store::open_memory().unwrap();
    let task = task(
        "t-copilot",
        AgentKind::Copilot,
        TaskStatus::Done,
        0,
        "gpt-5",
        None,
        Some(1_000),
        1_000,
    );
    store.insert_task(&task).unwrap();

    let stats = collect(&store, UsageWindow::Days(7), None, Local::now()).unwrap();

    assert_eq!(stats.agent_rows[0].cost, "subscription");
    assert_eq!(stats.model_rows[0].cost, "subscription");
}

#[test]
fn top_sessions_pick_correct_extremes() {
    let store = Store::open_memory().unwrap();
    for task in [task("t-long", AgentKind::Codex, TaskStatus::Done, 0, "gpt-5.4", Some(1.0), Some(300_000), 1_000), task("t-tokens", AgentKind::Codex, TaskStatus::Done, 0, "gpt-5.4", Some(0.5), Some(10_000), 3_100_000), task("t-cost", AgentKind::Codex, TaskStatus::Done, 0, "gpt-5.4", Some(11.91), Some(20_000), 2_000)] {
        store.insert_task(&task).unwrap();
    }
    let stats = collect(&store, UsageWindow::Days(7), None, Local::now()).unwrap();
    assert_eq!(stats.top_sessions.iter().find(|row| row.label == "Longest").unwrap().task_id, "t-long");
    assert_eq!(stats.top_sessions.iter().find(|row| row.label == "Most tokens").unwrap().task_id, "t-tokens");
    assert_eq!(stats.top_sessions.iter().find(|row| row.label == "Highest cost").unwrap().task_id, "t-cost");
}

#[test]
fn render_includes_overview_section() {
    let store = Store::open_memory().unwrap();
    store.insert_task(&task("t-1", AgentKind::Codex, TaskStatus::Done, 0, "gpt-5.4", Some(2.5), Some(1_000), 2_000)).unwrap();
    let output = render_output(&collect(&store, UsageWindow::Days(7), None, Local::now()).unwrap(), UsageWindow::Days(7), false, false);
    assert!(output.contains("Overview\n  Total: 1 tasks  2.0k tokens  $2.50"));
}

#[test]
fn render_includes_declared_difficulty_outcomes() {
    let store = Store::open_memory().unwrap();
    let task = task("t-declared", AgentKind::Codex, TaskStatus::Failed, 0, "gpt-5.4", Some(2.5), Some(120_000), 2_000);
    store.insert_task(&task).unwrap();
    store.update_task_profile("t-declared", crate::types::TaskProfileDeclaration {
        difficulty: Some(crate::types::TaskDifficulty::Simple),
        ..Default::default()
    }).unwrap();

    let stats = collect(&store, UsageWindow::Days(7), None, Local::now()).unwrap();
    let output = render_output(&stats, UsageWindow::Days(7), false, false);

    assert!(output.contains("Declared vs Outcome"));
    assert!(output.contains("simple"));
    assert!(output.contains("1 failed/verify-failed"));
}

#[test]
fn collects_usage_share_per_agent() {
    let store = Store::open_memory().unwrap();
    for task in [
        task("t-1", AgentKind::Codex, TaskStatus::Done, 0, "gpt-5.4", Some(1.0), Some(1_000), 1_000),
        task("t-2", AgentKind::Codex, TaskStatus::Done, 0, "gpt-5.4", Some(1.0), Some(1_000), 1_000),
        task("t-3", AgentKind::Codex, TaskStatus::Done, 0, "gpt-5.4", Some(1.0), Some(1_000), 1_000),
        task("t-4", AgentKind::OpenCode, TaskStatus::Done, 0, "glm-4.7", Some(0.1), Some(1_000), 1_000),
    ] {
        store.insert_task(&task).unwrap();
    }
    let stats = collect(&store, UsageWindow::Days(7), None, Local::now()).unwrap();
    assert_eq!(stats.agent_rows[0].share_pct, 75);
    assert_eq!(stats.agent_rows[1].share_pct, 25);
}

#[test]
fn unknown_model_cost_reads_as_unknown_not_free() {
    let store = Store::open_memory().unwrap();
    store
        .insert_task(&task(
            "t-unk",
            AgentKind::OpenCode,
            TaskStatus::Done,
            0,
            "not-a-real-model",
            None,
            Some(1_000),
            50_000,
        ))
        .unwrap();
    let stats = collect(&store, UsageWindow::Days(7), None, Local::now()).unwrap();
    assert_eq!(stats.model_rows[0].model, "not-a-real-model");
    assert_eq!(stats.model_rows[0].cost, "unknown");
    assert_eq!(stats.agent_rows[0].cost, "unknown");
    assert_eq!(stats.total_cost, None);
    let output = render_output(&stats, UsageWindow::Days(7), false, false);
    assert!(output.contains("unknown"));
    assert!(!output.contains("$0.00"));
}
