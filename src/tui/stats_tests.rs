// Literal aggregation tests for the TUI statistics snapshot.
// Covers day bucketing, range filtering, and project rollups.
// Deps: tui::stats and crate task types.

use super::*;
use crate::store::TaskStatsRow;
use chrono::{DateTime, Local, TimeZone};

fn task(id: &str, created: &str, completed: Option<&str>, repo: Option<&str>, tokens: Option<i64>) -> TaskStatsRow {
    TaskStatsRow {
        id: id.to_string(),
        created_at: at(created),
        completed_at: completed.map(at),
        repo_path: repo.map(str::to_string),
        tokens,
    }
}

fn at(value: &str) -> DateTime<Local> {
    Local.datetime_from_str(value, "%Y-%m-%d %H:%M:%S").expect("test timestamp")
}

#[test]
fn buckets_tasks_by_created_day_and_sums_recorded_tokens() {
    let tasks = vec![
        task("t-1", "2026-08-10 09:00:00", Some("2026-08-10 09:10:00"), Some("/repo/a"), Some(120)),
        task("t-2", "2026-08-10 12:00:00", None, Some("/repo/a"), None),
        task("t-3", "2026-08-11 12:00:00", Some("2026-08-11 12:20:00"), Some("/repo/b"), Some(80)),
    ];
    let snapshot = aggregate_tasks(&tasks, StatsRange::AllTime, at("2026-08-11 18:00:00"));

    assert_eq!(snapshot.total_tasks, 3);
    assert_eq!(snapshot.activity[0].date.to_string(), "2026-08-10");
    assert_eq!(snapshot.activity[0].task_count, 2);
    assert_eq!(snapshot.activity[0].tokens, 120);
    assert_eq!(snapshot.activity[1].task_count, 1);
    assert_eq!(snapshot.activity[1].tokens, 80);
}

#[test]
fn filters_tasks_to_selected_date_range() {
    let tasks = vec![
        task("t-old", "2026-07-01 09:00:00", Some("2026-07-01 09:10:00"), Some("/repo/a"), Some(10)),
        task("t-in", "2026-08-06 09:00:00", Some("2026-08-06 09:10:00"), Some("/repo/a"), Some(20)),
        task("t-today", "2026-08-12 09:00:00", None, Some("/repo/b"), Some(30)),
    ];
    let snapshot = aggregate_tasks(&tasks, StatsRange::Last7Days, at("2026-08-12 18:00:00"));

    assert_eq!(snapshot.total_tasks, 2);
    assert_eq!(snapshot.total_tokens, 50);
    assert_eq!(snapshot.activity.len(), 7);
    assert_eq!(snapshot.activity[0].date.to_string(), "2026-08-06");
    assert_eq!(snapshot.activity[6].task_count, 1);
}

#[test]
fn rolls_up_projects_and_ranks_by_task_count_then_tokens() {
    let tasks = vec![
        task("t-1", "2026-08-10 09:00:00", Some("2026-08-10 09:01:00"), Some("/repo/a"), Some(10)),
        task("t-2", "2026-08-10 10:00:00", Some("2026-08-10 10:03:00"), Some("/repo/a"), Some(30)),
        task("t-3", "2026-08-10 11:00:00", Some("2026-08-10 11:02:00"), Some("/repo/b"), Some(100)),
        task("t-4", "2026-08-10 12:00:00", None, None, None),
    ];
    let snapshot = aggregate_tasks(&tasks, StatsRange::AllTime, at("2026-08-10 18:00:00"));

    assert_eq!(snapshot.projects[0].name, "/repo/a");
    assert_eq!(snapshot.projects[0].task_count, 2);
    assert_eq!(snapshot.projects[0].tokens, 40);
    assert_eq!(snapshot.projects[0].duration_secs, 240);
    assert_eq!(snapshot.projects[1].name, "/repo/b");
    assert_eq!(snapshot.projects[2].name, "(no repo_path)");
    assert_eq!(snapshot.projects[2].token_task_count, 0);
}

#[test]
fn range_selector_cycles_in_display_order() {
    assert_eq!(StatsRange::AllTime.next(), StatsRange::Last30Days);
    assert_eq!(StatsRange::Last30Days.next(), StatsRange::Last7Days);
    assert_eq!(StatsRange::Last7Days.next(), StatsRange::Today);
    assert_eq!(StatsRange::Today.next(), StatsRange::AllTime);
    assert_eq!(StatsRange::AllTime.previous(), StatsRange::Today);
    assert_eq!(StatsRange::Today.label(), "Today");
}
