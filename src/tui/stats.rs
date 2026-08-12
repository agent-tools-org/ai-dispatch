// Refresh-time statistics aggregation for the aid TUI.
// Exports: StatsRange, StatsSnapshot, and aggregate_tasks.
// Deps: chrono, task records from crate::types, and std collections.

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate};
use std::collections::HashMap;

use crate::types::Task;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsRange {
    AllTime,
    Last30Days,
    Last7Days,
    Today,
}

impl StatsRange {
    pub fn label(self) -> &'static str {
        match self {
            Self::AllTime => "All time",
            Self::Last30Days => "Last 30 days",
            Self::Last7Days => "Last 7 days",
            Self::Today => "Today",
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::AllTime => Self::Today,
            Self::Last30Days => Self::AllTime,
            Self::Last7Days => Self::Last30Days,
            Self::Today => Self::Last7Days,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::AllTime => Self::Last30Days,
            Self::Last30Days => Self::Last7Days,
            Self::Last7Days => Self::Today,
            Self::Today => Self::AllTime,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyStats {
    pub date: NaiveDate,
    pub weekday: u32,
    pub task_count: usize,
    pub tokens: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectStats {
    pub name: String,
    pub task_count: usize,
    pub tokens: i64,
    pub token_task_count: usize,
    pub duration_secs: i64,
    pub duration_task_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatsSnapshot {
    pub range: StatsRange,
    pub total_tasks: usize,
    pub total_tokens: i64,
    pub token_task_count: usize,
    pub total_duration_secs: i64,
    pub duration_task_count: usize,
    pub activity: Vec<DailyStats>,
    pub activity_max: usize,
    pub token_trend: Vec<u64>,
    pub token_max: u64,
    pub projects: Vec<ProjectStats>,
    pub current_streak: usize,
    pub best_streak: usize,
    pub peak_tokens: Option<(NaiveDate, i64)>,
}

#[derive(Default)]
struct AggregateTotals {
    total_tasks: usize,
    total_tokens: i64,
    token_task_count: usize,
    total_duration_secs: i64,
    duration_task_count: usize,
}

impl StatsSnapshot {
    pub fn empty(range: StatsRange) -> Self {
        Self {
            range,
            total_tasks: 0,
            total_tokens: 0,
            token_task_count: 0,
            total_duration_secs: 0,
            duration_task_count: 0,
            activity: Vec::new(),
            activity_max: 0,
            token_trend: Vec::new(),
            token_max: 1,
            projects: Vec::new(),
            current_streak: 0,
            best_streak: 0,
            peak_tokens: None,
        }
    }
}

pub fn aggregate_tasks(tasks: &[Task], range: StatsRange, now: DateTime<Local>) -> StatsSnapshot {
    let end = now.date_naive();
    let start = range_start(tasks, range, end);
    let days = (end - start).num_days().max(0) as usize + 1;
    let mut activity = (0..days)
        .map(|offset| {
            let date = start + Duration::days(offset as i64);
            DailyStats { date, weekday: date.weekday().num_days_from_sunday(), task_count: 0, tokens: 0 }
        })
        .collect::<Vec<_>>();
    let mut projects = HashMap::<String, ProjectStats>::new();
    let mut totals = AggregateTotals::default();
    for task in tasks.iter().filter(|task| in_range(task, start, end)) {
        record_task(task, start, &mut activity, &mut projects, &mut totals);
    }
    let mut projects = projects.into_values().collect::<Vec<_>>();
    projects.sort_by(|left, right| right.task_count.cmp(&left.task_count).then_with(|| right.tokens.cmp(&left.tokens)).then_with(|| left.name.cmp(&right.name)));
    let activity_max = activity.iter().map(|day| day.task_count).max().unwrap_or(0);
    let token_trend = activity.iter().map(|day| day.tokens.max(0) as u64).collect::<Vec<_>>();
    let token_max = token_trend.iter().copied().max().unwrap_or(1).max(1);
    let peak_tokens = activity.iter().filter(|day| day.tokens != 0).max_by_key(|day| day.tokens).map(|day| (day.date, day.tokens));
    let (current_streak, best_streak) = streaks(&activity);
    StatsSnapshot { range, total_tasks: totals.total_tasks, total_tokens: totals.total_tokens, token_task_count: totals.token_task_count, total_duration_secs: totals.total_duration_secs, duration_task_count: totals.duration_task_count, activity, activity_max, token_trend, token_max, projects, current_streak, best_streak, peak_tokens }
}

fn record_task(task: &Task, start: NaiveDate, activity: &mut [DailyStats], projects: &mut HashMap<String, ProjectStats>, totals: &mut AggregateTotals) {
    totals.total_tasks += 1;
    let index = (task.created_at.date_naive() - start).num_days() as usize;
    if let Some(day) = activity.get_mut(index) {
        day.task_count += 1;
        if let Some(tokens) = task.tokens { day.tokens += tokens; }
    }
    let project_name = task.repo_path.clone().unwrap_or_else(|| "(no repo_path)".to_string());
    let project = projects.entry(project_name.clone()).or_insert_with(|| ProjectStats {
        name: project_name, task_count: 0, tokens: 0, token_task_count: 0,
        duration_secs: 0, duration_task_count: 0,
    });
    project.task_count += 1;
    if let Some(tokens) = task.tokens {
        totals.total_tokens += tokens;
        totals.token_task_count += 1;
        project.tokens += tokens;
        project.token_task_count += 1;
    }
    if let Some(seconds) = task_duration_secs(task) {
        totals.total_duration_secs += seconds;
        totals.duration_task_count += 1;
        project.duration_secs += seconds;
        project.duration_task_count += 1;
    }
}

fn range_start(tasks: &[Task], range: StatsRange, end: NaiveDate) -> NaiveDate {
    match range {
        StatsRange::AllTime => tasks.iter().map(|task| task.created_at.date_naive()).filter(|date| *date <= end).min().unwrap_or(end),
        StatsRange::Last30Days => end - Duration::days(29),
        StatsRange::Last7Days => end - Duration::days(6),
        StatsRange::Today => end,
    }
}

fn in_range(task: &Task, start: NaiveDate, end: NaiveDate) -> bool {
    let date = task.created_at.date_naive();
    date >= start && date <= end
}

fn task_duration_secs(task: &Task) -> Option<i64> {
    let completed_at = task.completed_at?;
    let seconds = (completed_at - task.created_at).num_seconds();
    (seconds >= 0).then_some(seconds)
}

fn streaks(days: &[DailyStats]) -> (usize, usize) {
    let mut best = 0;
    let mut run = 0;
    for day in days {
        if day.task_count > 0 { run += 1; best = best.max(run); } else { run = 0; }
    }
    let current = days.iter().rev().take_while(|day| day.task_count > 0).count();
    (current, best)
}

pub fn format_tokens(tokens: i64) -> String {
    if tokens >= 1_000_000 { format!("{:.1}M", tokens as f64 / 1_000_000.0) } else if tokens >= 1_000 { format!("{:.1}k", tokens as f64 / 1_000.0) } else { tokens.to_string() }
}

pub fn format_duration(seconds: i64) -> String {
    if seconds >= 3_600 { format!("{}h {:02}m", seconds / 3_600, (seconds % 3_600) / 60) } else if seconds >= 60 { format!("{}m {:02}s", seconds / 60, seconds % 60) } else { format!("{seconds}s") }
}

#[cfg(test)]
#[path = "stats_tests.rs"]
mod tests;
