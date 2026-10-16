// Usage execution statistics and trend regression tests.
// Shares synthetic task fixtures with the usage budget tests.
use super::tests::make_task;
use super::{UsageWindow, agent_analytics, collect_usage_from_tasks};
use crate::config::AidConfig;
use crate::cost::usage_report::render_agent_analytics;
use crate::types::{AgentKind, TaskStatus, VerifyStatus};
use chrono::{Duration, Local};

#[test]
fn calculates_agent_execution_stats() {
    let mut done_task = make_task("t-done", AgentKind::Codex, 12_000, 0.45);
    done_task.duration_ms = Some(30_000);

    let mut failed_task = make_task("t-failed", AgentKind::Codex, 1_000, 0.05);
    failed_task.status = TaskStatus::Failed;
    failed_task.duration_ms = Some(90_000);

    let mut pending_retry = make_task("t-retry", AgentKind::Codex, 0, 0.0);
    pending_retry.status = TaskStatus::Pending;
    pending_retry.tokens = None;
    pending_retry.cost_usd = None;
    pending_retry.duration_ms = None;
    pending_retry.parent_task_id = Some("t-failed".to_string());

    let snapshot =
        collect_usage_from_tasks(&[done_task, failed_task, pending_retry], &AidConfig::default()).unwrap();
    let row = snapshot
        .agent_rows
        .iter()
        .find(|row| row.name == AgentKind::Codex.as_str())
        .unwrap();

    assert_eq!(row.tasks, 3);
    assert_eq!(row.retry_count, 1);
    assert!((row.success_rate - 33.333333333333336).abs() < 0.0001);
    assert_eq!(row.avg_duration_secs, 60.0);
}

#[test]
fn timed_out_verification_is_not_counted_as_usage_success() {
    let done = make_task("t-done", AgentKind::Codex, 1_000, 0.10);
    let mut timed_out = make_task("t-timeout", AgentKind::Codex, 1_000, 0.10);
    timed_out.verify = Some("cargo test".to_string());
    timed_out.verify_status = VerifyStatus::TimedOut;

    let snapshot = collect_usage_from_tasks(&[done, timed_out], &AidConfig::default()).unwrap();
    let row = snapshot
        .agent_rows
        .iter()
        .find(|row| row.name == AgentKind::Codex.as_str())
        .unwrap();

    assert_eq!(row.success_rate, 50.0);
}

#[test]
fn stopped_tasks_are_excluded_from_usage_success_rate() {
    let done = make_task("t-done", AgentKind::Codex, 1_000, 0.10);
    let mut stopped = make_task("t-stopped", AgentKind::Codex, 1_000, 0.10);
    stopped.status = TaskStatus::Stopped;

    let snapshot = collect_usage_from_tasks(&[done, stopped], &AidConfig::default()).unwrap();
    let row = snapshot
        .agent_rows
        .iter()
        .find(|row| row.name == AgentKind::Codex.as_str())
        .unwrap();

    assert_eq!(row.tasks, 2);
    assert_eq!(row.success_rate, 100.0);
}

#[test]
fn usage_window_parses_last_days() {
    let window = UsageWindow::parse("7d").unwrap();
    let now = Local::now();
    let range = window.range(now).unwrap();
    assert_eq!(range.1, now);
    assert_eq!(now - range.0, Duration::days(7));
}

#[test]
fn agent_analytics_trend_and_top_tasks() {
    let now = Local::now();
    let mut current = make_task("t-current", AgentKind::Codex, 1_000, 0.55);
    current.status = TaskStatus::Done;
    current.duration_ms = Some(60_000);
    current.created_at = now - Duration::days(1);

    let mut previous = make_task("t-previous", AgentKind::Codex, 2_000, 1.25);
    previous.status = TaskStatus::Done;
    previous.duration_ms = Some(90_000);
    previous.created_at = now - Duration::days(8);

    let tasks = vec![current.clone(), previous.clone()];
    let window = UsageWindow::parse("7d").unwrap();
    let analytics = agent_analytics(&tasks, "codex", window, now);

    assert_eq!(analytics.stats.tasks, 1);
    assert_eq!(analytics.stats.success_count, 1);
    assert!(analytics.cost_per_success.is_some());
    let trend = analytics.trend.as_ref().unwrap();
    assert_eq!(trend.previous.tasks, 1);
    assert_eq!(trend.previous.cost_usd, previous.cost_usd.unwrap());
    assert_eq!(analytics.top_tasks.len(), 1);
    let rendered = render_agent_analytics(&analytics);
    assert!(rendered.contains("Top 5 most expensive tasks"));
}
