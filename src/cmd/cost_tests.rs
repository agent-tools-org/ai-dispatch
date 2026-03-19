// Tests for `aid cost` aggregation helpers.
// Covers workgroup totals, daily summary rows, and empty results.

use chrono::{Duration, Local};
use std::sync::Arc;

use super::cost::{daily_summary_rows, group_tasks, run};
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use crate::usage::UsageWindow;

fn make_task(
    id: &str,
    agent: AgentKind,
    group: Option<&str>,
    created_at: chrono::DateTime<Local>,
    tokens: i64,
    cost_usd: f64,
) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: group.map(str::to_string),
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        worktree_path: None,
        worktree_branch: None,
        log_path: None,
        output_path: None,
        tokens: Some(tokens),
        prompt_tokens: None,
        duration_ms: Some(1_000),
        model: None,
        cost_usd: Some(cost_usd),
        exit_code: None,
        created_at,
        completed_at: None,
        verify: None,
        verify_status: VerifyStatus::Skipped,
        read_only: false,
        budget: false,
    }
}

#[test]
fn aggregates_group_costs() {
    let now = Local::now();
    let tasks = vec![
        make_task("t-1", AgentKind::Codex, Some("wg-1"), now, 100, 1.25),
        make_task("t-2", AgentKind::Gemini, Some("wg-1"), now, 200, 0.75),
        make_task("t-3", AgentKind::Cursor, Some("wg-2"), now, 999, 9.99),
    ];

    let rows = group_tasks(&tasks, "wg-1");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows.iter().filter_map(|task| task.tokens).sum::<i64>(), 300);
    assert!((rows.iter().filter_map(|task| task.cost_usd).sum::<f64>() - 2.0).abs() < 0.0001);
}

#[test]
fn aggregates_daily_summary_rows() {
    let now = Local::now();
    let tasks = vec![
        make_task("t-1", AgentKind::Codex, None, now, 100, 1.25),
        make_task("t-2", AgentKind::Gemini, None, now, 200, 0.75),
        make_task("t-3", AgentKind::Cursor, None, now - Duration::days(2), 300, 2.50),
        make_task("t-4", AgentKind::Cursor, None, now - Duration::days(10), 400, 4.50),
    ];

    let (rows, totals) = daily_summary_rows(&tasks, UsageWindow::Days(7), now);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].tasks, 2);
    assert_eq!(rows[0].tokens, 300);
    assert!((rows[0].cost_usd - 2.0).abs() < 0.0001);
    assert_eq!(totals.0, 3);
    assert_eq!(totals.1, 600);
    assert!((totals.2 - 4.5).abs() < 0.0001);
}

#[test]
fn empty_workgroup_returns_no_tasks() {
    let tasks = vec![make_task(
        "t-1",
        AgentKind::Codex,
        Some("wg-1"),
        Local::now(),
        100,
        1.25,
    )];
    let rows = group_tasks(&tasks, "wg-missing");
    assert!(rows.is_empty());
}

#[test]
fn run_accepts_empty_group_report() {
    let store = Arc::new(Store::open_memory().unwrap());
    assert!(run(&store, Some("wg-missing".to_string()), false, None, "7d".to_string()).is_ok());
}
