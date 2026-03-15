// Usage and cost snapshots built from task history plus config budgets.
// Exports collect_usage() and render_usage() for the `aid usage` command.

use anyhow::Result;
use chrono::{DateTime, Duration, Local, LocalResult, TimeZone};
use serde::Serialize;
use std::cmp::Ordering;

use crate::config::{AidConfig, UsageBudget};
use crate::cost;
use crate::paths;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskFilter, TaskStatus};

pub struct BudgetStatus {
    pub over_limit: bool,
    pub near_limit: bool,
    pub message: Option<String>,
}

#[derive(Serialize)]
pub struct UsageSnapshot {
    generated_at: DateTime<Local>,
    agent_rows: Vec<AgentUsageRow>,
    budget_rows: Vec<BudgetUsageRow>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum UsageWindow {
    All,
    Today,
    Days(u32),
}

impl Default for UsageWindow {
    fn default() -> Self {
        UsageWindow::All
    }
}

impl UsageWindow {
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_lowercase().as_str() {
            "all" => Ok(Self::All),
            "today" => Ok(Self::Today),
            "7d" => Ok(Self::Days(7)),
            "30d" => Ok(Self::Days(30)),
            other => anyhow::bail!("Unknown period '{}'", other),
        }
    }

    pub fn range(&self, now: DateTime<Local>) -> Option<(DateTime<Local>, DateTime<Local>)> {
        match self {
            Self::All => None,
            Self::Today => Some((start_of_day(now), now)),
            Self::Days(days) => {
                let duration = Duration::days(*days as i64);
                Some((now - duration, now))
            }
        }
    }

    pub fn previous_range(&self, now: DateTime<Local>) -> Option<(DateTime<Local>, DateTime<Local>)> {
        if let Some(duration) = self.duration() {
            let (start, _) = self.range(now)?;
            Some((start - duration, start))
        } else {
            None
        }
    }

    fn duration(&self) -> Option<Duration> {
        match self {
            Self::All => None,
            Self::Today => Some(Duration::days(1)),
            Self::Days(days) => Some(Duration::days(*days as i64)),
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::All => "all time",
            Self::Today => "today",
            Self::Days(7) => "last 7 days",
            Self::Days(30) => "last 30 days",
            Self::Days(_) => "custom window",
        }
    }

    pub fn previous_label(&self) -> String {
        format!("previous {}", self.description())
    }
}

#[derive(Serialize)]
pub struct AgentAnalytics {
    pub agent_name: String,
    pub window: UsageWindow,
    pub stats: AgentPeriodStats,
    pub cost_per_success: Option<f64>,
    pub trend: Option<AgentTrend>,
    pub top_tasks: Vec<TaskSummary>,
}

#[derive(Serialize)]
pub struct AgentPeriodStats {
    pub tasks: usize,
    pub success_count: usize,
    pub fail_count: usize,
    pub retry_count: usize,
    pub tokens: i64,
    pub cost_usd: f64,
    pub avg_duration_secs: f64,
}

#[derive(Serialize)]
pub struct AgentTrend {
    pub label: String,
    pub current: AgentTrendStats,
    pub previous: AgentTrendStats,
}

#[derive(Serialize)]
pub struct AgentTrendStats {
    pub tasks: usize,
    pub cost_usd: f64,
}

#[derive(Serialize)]
pub struct TaskSummary {
    pub id: String,
    pub prompt_snippet: String,
    pub cost_usd: f64,
    pub duration_secs: Option<f64>,
}

#[derive(Serialize)]
#[serde(tag = "view", rename_all = "snake_case")]
pub enum UsageReport {
    Summary {
        window: UsageWindow,
        snapshot: UsageSnapshot,
    },
    Agent {
        analytics: AgentAnalytics,
    },
}

#[derive(Serialize)]
struct AgentUsageRow {
    name: &'static str,
    tasks: usize,
    tokens: i64,
    cost_usd: f64,
    success_rate: f64,
    avg_duration_secs: f64,
    retry_count: usize,
    last_task_at: Option<DateTime<Local>>,
}

#[derive(Serialize)]
struct BudgetUsageRow {
    name: String,
    plan: Option<String>,
    window: Option<String>,
    tasks: u32,
    task_limit: Option<u32>,
    tokens: i64,
    token_limit: Option<i64>,
    cost_usd: f64,
    cost_limit_usd: Option<f64>,
    requests: u32,
    request_limit: Option<u32>,
    resets_at: Option<String>,
    notes: Option<String>,
}

pub fn collect_usage(store: &Store, config: &AidConfig) -> Result<UsageSnapshot> {
    let tasks = store.list_tasks(TaskFilter::All)?;
    collect_usage_from_tasks(&tasks, config)
}

pub fn collect_usage_from_tasks(tasks: &[Task], config: &AidConfig) -> Result<UsageSnapshot> {
    collect_usage_snapshot(tasks, config, UsageWindow::All, Local::now())
}

pub fn collect_usage_snapshot(
    tasks: &[Task],
    config: &AidConfig,
    window: UsageWindow,
    now: DateTime<Local>,
) -> Result<UsageSnapshot> {
    let filtered_tasks = filter_tasks_by_window(tasks, window, now);
    Ok(UsageSnapshot {
        generated_at: now,
        agent_rows: collect_agent_rows(&filtered_tasks),
        budget_rows: collect_budget_rows(tasks, &config.usage.budgets),
    })
}

pub fn check_budget_status(store: &Store, config: &AidConfig) -> Result<BudgetStatus> {
    let tasks = store.list_tasks(TaskFilter::All)?;
    let budget_rows = collect_budget_rows(&tasks, &config.usage.budgets);

    let mut over_limit = false;
    let mut near_limit = false;
    let mut messages: Vec<String> = Vec::new();

    for row in &budget_rows {
        if let Some(limit) = row.task_limit {
            if row.tasks >= limit {
                over_limit = true;
                messages.push(format!(
                    "Budget '{}': task limit reached ({}/{})",
                    row.name, row.tasks, limit
                ));
            } else if row.tasks as f64 / limit as f64 > 0.8 {
                near_limit = true;
                messages.push(format!(
                    "Budget '{}': task usage at {}%",
                    row.name,
                    (row.tasks as f64 / limit as f64 * 100.0) as u32
                ));
            }
        }
        if let Some(limit) = row.cost_limit_usd {
            if row.cost_usd >= limit {
                over_limit = true;
                messages.push(format!(
                    "Budget '{}': cost limit reached (${:.2}/${:.2})",
                    row.name, row.cost_usd, limit
                ));
            } else if row.cost_usd / limit > 0.8 {
                near_limit = true;
                messages.push(format!(
                    "Budget '{}': cost usage at {}%",
                    row.name,
                    (row.cost_usd / limit * 100.0) as u32
                ));
            }
        }
    }

    Ok(BudgetStatus {
        over_limit,
        near_limit,
        message: if messages.is_empty() {
            None
        } else {
            Some(messages.join("\n"))
        },
    })
}

pub fn render_usage(snapshot: &UsageSnapshot) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Usage snapshot at {}\n",
        snapshot.generated_at.format("%Y-%m-%d %H:%M:%S %:z"),
    ));

    if !snapshot.agent_rows.is_empty() {
        out.push_str("\nTracked Task History\n");
        out.push_str(&format!(
            "{:<12} {:<8} {:<10} {:<10} {:<6} {:<8} {:<7} {}\n",
            "Agent", "Tasks", "Tokens", "Cost", "Success%", "Avg Time", "Retries", "Last Task"
        ));
        out.push_str(&"-".repeat(80));
        out.push('\n');
        for row in &snapshot.agent_rows {
            out.push_str(&format!(
                "{:<12} {:<8} {:<10} {:<10} {:<6} {:<8} {:<7} {}\n",
                row.name,
                row.tasks,
                format_tokens(row.tokens),
                cost::format_cost(Some(row.cost_usd)),
                format!("{:.1}%", row.success_rate),
                format_duration_secs(row.avg_duration_secs),
                row.retry_count,
                row.last_task_at
                    .map(format_last_seen)
                    .unwrap_or_else(|| "-".to_string()),
            ));
        }
    }

    if snapshot.budget_rows.is_empty() {
        out.push_str(&format!(
            "\nNo budgets configured. Add `[[usage.budget]]` entries to {}.\n",
            paths::config_path().display(),
        ));
        return out;
    }

    out.push_str("\nConfigured Budgets\n");
    out.push_str(&format!(
        "{:<16} {:<8} {:<8} {:<12} {:<14} {:<14} {:<14} {}\n",
        "Name", "Plan", "Window", "Tasks", "Tokens", "Cost", "Requests", "Resets"
    ));
    out.push_str(&"-".repeat(118));
    out.push('\n');
    for row in &snapshot.budget_rows {
        out.push_str(&format!(
            "{:<16} {:<8} {:<8} {:<12} {:<14} {:<14} {:<14} {}\n",
            row.name,
            row.plan.as_deref().unwrap_or("-"),
            row.window.as_deref().unwrap_or("-"),
            format_ratio_u32(row.tasks, row.task_limit),
            format_ratio_i64(row.tokens, row.token_limit, format_tokens),
            format_ratio_f64(row.cost_usd, row.cost_limit_usd),
            format_ratio_u32(row.requests, row.request_limit),
            row.resets_at.as_deref().unwrap_or("-"),
        ));
        if let Some(notes) = row.notes.as_deref() {
            out.push_str(&format!("  note: {notes}\n"));
        }
    }
    out
}

pub fn agent_analytics(
    tasks: &[Task],
    agent_filter: &str,
    window: UsageWindow,
    now: DateTime<Local>,
) -> AgentAnalytics {
    let agent_key = agent_filter.trim();
    let matching_tasks: Vec<&Task> = tasks
        .iter()
        .filter(|task| task.agent_display_name().eq_ignore_ascii_case(agent_key))
        .collect();
    let display_name = matching_tasks
        .first()
        .map(|task| task.agent_display_name().to_string())
        .unwrap_or_else(|| agent_key.to_lowercase());
    let current_range = window.range(now);
    let current_tasks = filter_tasks_in_range(&matching_tasks, current_range);
    let stats = summarize_agent_period(&current_tasks);
    let cost_per_success = (stats.success_count > 0)
        .then(|| stats.cost_usd / stats.success_count as f64);
    let trend = window.previous_range(now).and_then(|range| {
        let previous_tasks = filter_tasks_in_range(&matching_tasks, Some(range));
        let previous_stats = summarize_agent_period(&previous_tasks);
        Some(AgentTrend {
            label: window.previous_label(),
            current: AgentTrendStats {
                tasks: stats.tasks,
                cost_usd: stats.cost_usd,
            },
            previous: AgentTrendStats {
                tasks: previous_stats.tasks,
                cost_usd: previous_stats.cost_usd,
            },
        })
    });
    let top_tasks = select_top_tasks(&current_tasks);
    AgentAnalytics {
        agent_name: display_name,
        window,
        stats,
        cost_per_success,
        trend,
        top_tasks,
    }
}

pub fn render_agent_analytics(analytics: &AgentAnalytics) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Agent '{agent}' usage ({period})\n",
        agent = analytics.agent_name,
        period = analytics.window.description()
    ));
    out.push_str(&format!(
        "Tasks: {} (success {}, fail {}, retries {})\n",
        analytics.stats.tasks,
        analytics.stats.success_count,
        analytics.stats.fail_count,
        analytics.stats.retry_count
    ));
    out.push_str(&format!(
        "Tokens: {} | Cost: {} | Avg duration: {}\n",
        format_tokens(analytics.stats.tokens),
        cost::format_cost(Some(analytics.stats.cost_usd)),
        format_duration_secs(analytics.stats.avg_duration_secs)
    ));
    let cost_per_success = analytics
        .cost_per_success
        .map(|cost| cost::format_cost(Some(cost)))
        .unwrap_or_else(|| "-".to_string());
    out.push_str(&format!("Cost per success: {}\n", cost_per_success));
    if let Some(trend) = &analytics.trend {
        let task_delta = trend.current.tasks as isize - trend.previous.tasks as isize;
        let cost_delta = trend.current.cost_usd - trend.previous.cost_usd;
        let cost_delta_label = format!(
            "{}{}",
            if cost_delta >= 0.0 { '+' } else { '-' },
            cost::format_cost(Some(cost_delta.abs()))
        );
        out.push_str(&format!(
            "Trend vs {}: tasks {:+}, cost {}\n",
            trend.label, task_delta, cost_delta_label
        ));
    }
    out.push_str("Top 5 most expensive tasks:\n");
    if analytics.top_tasks.is_empty() {
        out.push_str("  (no tasks recorded in this period)\n");
    } else {
        for task in &analytics.top_tasks {
            let duration = task
                .duration_secs
                .map(format_duration_secs)
                .unwrap_or_else(|| "-".to_string());
            out.push_str(&format!(
                "  {} | {} | {} | {}\n",
                task.id,
                cost::format_cost(Some(task.cost_usd)),
                duration,
                task.prompt_snippet
            ));
        }
    }
    out
}

fn summarize_agent_period(tasks: &[&Task]) -> AgentPeriodStats {
    let success_count = tasks
        .iter()
        .filter(|task| matches!(task.status, TaskStatus::Done | TaskStatus::Merged))
        .count();
    let fail_count = tasks
        .iter()
        .filter(|task| matches!(task.status, TaskStatus::Failed))
        .count();
    let retry_count = tasks.iter().filter(|task| task.parent_task_id.is_some()).count();
    let tokens = tasks.iter().map(|task| task.tokens.unwrap_or(0)).sum();
    let cost_usd = tasks.iter().map(|task| task.cost_usd.unwrap_or(0.0)).sum();
    let durations: Vec<i64> = tasks
        .iter()
        .filter(|task| task.status.is_terminal())
        .filter_map(|task| task.duration_ms)
        .collect();
    let avg_duration_secs = if durations.is_empty() {
        0.0
    } else {
        durations.iter().sum::<i64>() as f64 / durations.len() as f64 / 1000.0
    };
    AgentPeriodStats {
        tasks: tasks.len(),
        success_count,
        fail_count,
        retry_count,
        tokens,
        cost_usd,
        avg_duration_secs,
    }
}

fn filter_tasks_in_range<'a>(
    tasks: &'a [&'a Task],
    range: Option<(DateTime<Local>, DateTime<Local>)>,
) -> Vec<&'a Task> {
    match range {
        Some((start, end)) => tasks
            .iter()
            .copied()
            .filter(|task| task.created_at >= start && task.created_at <= end)
            .collect(),
        None => tasks.to_vec(),
    }
}

fn select_top_tasks(tasks: &[&Task]) -> Vec<TaskSummary> {
    let mut top: Vec<&Task> = tasks.to_vec();
    top.sort_by(|a, b| {
        let a_cost = a.cost_usd.unwrap_or(0.0);
        let b_cost = b.cost_usd.unwrap_or(0.0);
        b_cost
            .partial_cmp(&a_cost)
            .unwrap_or(Ordering::Equal)
    });
    top.truncate(5);
    top.into_iter()
        .map(|task| TaskSummary {
            id: task.id.to_string(),
            prompt_snippet: prompt_snippet(task),
            cost_usd: task.cost_usd.unwrap_or(0.0),
            duration_secs: task.duration_ms.map(|ms| ms as f64 / 1000.0),
        })
        .collect()
}

fn prompt_snippet(task: &Task) -> String {
    let source = task.resolved_prompt.as_deref().unwrap_or(&task.prompt);
    let trimmed = source.lines().next().unwrap_or("").trim();
    if trimmed.chars().count() <= 80 {
        trimmed.to_string()
    } else {
        trimmed.chars().take(77).collect::<String>() + "..."
    }
}

fn collect_agent_rows(tasks: &[Task]) -> Vec<AgentUsageRow> {
    let mut rows = Vec::new();
    for agent in [
        AgentKind::Codex,
        AgentKind::Gemini,
        AgentKind::OpenCode,
        AgentKind::Kilo,
        AgentKind::Cursor,
        AgentKind::Ob1,
    ] {
        let agent_tasks: Vec<&Task> = tasks.iter().filter(|task| task.agent == agent).collect();
        if agent_tasks.is_empty() {
            continue;
        }

        let done_count = agent_tasks
            .iter()
            .filter(|task| matches!(task.status, TaskStatus::Done | TaskStatus::Merged))
            .count();
        let retry_count = agent_tasks
            .iter()
            .filter(|task| task.parent_task_id.is_some())
            .count();
        let completed_durations: Vec<i64> = agent_tasks
            .iter()
            .filter(|task| task.status.is_terminal())
            .filter_map(|task| task.duration_ms)
            .collect();
        let success_rate = (done_count as f64 * 100.0) / agent_tasks.len() as f64;
        let avg_duration_secs = if completed_durations.is_empty() {
            0.0
        } else {
            completed_durations.iter().sum::<i64>() as f64
                / completed_durations.len() as f64
                / 1000.0
        };
        let tokens = agent_tasks.iter().filter_map(|task| task.tokens).sum();
        let cost_usd = agent_tasks.iter().filter_map(|task| task.cost_usd).sum();
        let last_task_at = agent_tasks.iter().map(|task| task.created_at).max();
        rows.push(AgentUsageRow {
            name: agent.as_str(),
            tasks: agent_tasks.len(),
            tokens,
            cost_usd,
            success_rate,
            avg_duration_secs,
            retry_count,
            last_task_at,
        });
    }
    rows
}

fn collect_budget_rows(tasks: &[Task], budgets: &[UsageBudget]) -> Vec<BudgetUsageRow> {
    budgets
        .iter()
        .map(|budget| {
            let budget_tasks = filter_budget_tasks(tasks, budget);
            let tasks_used = budget_tasks.len() as u32 + budget.external_tasks;
            let tokens_used = budget_tasks
                .iter()
                .filter_map(|task| task.tokens)
                .sum::<i64>()
                + budget.external_tokens;
            let cost_used = budget_tasks
                .iter()
                .filter_map(|task| task.cost_usd)
                .sum::<f64>()
                + budget.external_cost_usd;
            BudgetUsageRow {
                name: budget.name.clone(),
                plan: budget.plan.clone(),
                window: budget.window.clone(),
                tasks: tasks_used,
                task_limit: budget.task_limit,
                tokens: tokens_used,
                token_limit: budget.token_limit,
                cost_usd: cost_used,
                cost_limit_usd: budget.cost_limit_usd,
                requests: budget.external_requests,
                request_limit: budget.request_limit,
                resets_at: budget.resets_at.clone(),
                notes: budget.notes.clone(),
            }
        })
        .collect()
}

fn filter_tasks_by_window(tasks: &[Task], window: UsageWindow, now: DateTime<Local>) -> Vec<Task> {
    match window.range(now) {
        None => tasks.to_vec(),
        Some((start, end)) => tasks
            .iter()
            .filter(|task| task.created_at >= start && task.created_at <= end)
            .cloned()
            .collect(),
    }
}

fn filter_budget_tasks<'a>(tasks: &'a [Task], budget: &UsageBudget) -> Vec<&'a Task> {
    let window_start = budget
        .window
        .as_deref()
        .and_then(parse_window)
        .map(|window| Local::now() - window);

    tasks
        .iter()
        .filter(|task| {
            let agent_matches = budget
                .agent
                .as_deref()
                .map(|name| task.agent_display_name() == name)
                .unwrap_or(false);
            let window_matches = window_start
                .map(|start| task.created_at >= start)
                .unwrap_or(true);
            agent_matches && window_matches
        })
        .collect()
}

fn parse_window(value: &str) -> Option<Duration> {
    let trimmed = value.trim();
    if let Some(hours) = trimmed.strip_suffix('h') {
        return hours.parse::<i64>().ok().map(Duration::hours);
    }
    if let Some(days) = trimmed.strip_suffix('d') {
        return days.parse::<i64>().ok().map(Duration::days);
    }
    if let Some(minutes) = trimmed.strip_suffix('m') {
        return minutes.parse::<i64>().ok().map(Duration::minutes);
    }
    None
}

fn start_of_day(now: DateTime<Local>) -> DateTime<Local> {
    let date = now.date_naive();
    let naive = date.and_hms_opt(0, 0, 0).unwrap();
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(dt) => dt,
        LocalResult::Ambiguous(dt, _) => dt,
        LocalResult::None => Local.from_utc_datetime(&naive),
    }
}

fn format_last_seen(timestamp: DateTime<Local>) -> String {
    let elapsed = Local::now() - timestamp;
    let secs = elapsed.num_seconds();
    if secs < 60 {
        format!("{secs}s ago")
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else {
        format!("{}h ago", secs / 3600)
    }
}

fn format_duration_secs(seconds: f64) -> String {
    let secs = seconds.round() as i64;
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m {:02}s", secs / 60, secs % 60)
    }
}

fn format_tokens(tokens: i64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

fn format_ratio_u32(current: u32, limit: Option<u32>) -> String {
    limit
        .map(|limit| format!("{current}/{limit}"))
        .unwrap_or_else(|| current.to_string())
}

fn format_ratio_i64(current: i64, limit: Option<i64>, formatter: fn(i64) -> String) -> String {
    limit
        .map(|limit| format!("{}/{}", formatter(current), formatter(limit)))
        .unwrap_or_else(|| formatter(current))
}

fn format_ratio_f64(current: f64, limit: Option<f64>) -> String {
    limit
        .map(|limit| {
            format!(
                "{}/{}",
                cost::format_cost(Some(current)),
                cost::format_cost(Some(limit))
            )
        })
        .unwrap_or_else(|| cost::format_cost(Some(current)))
}

#[cfg(test)]
mod tests {
    use super::{
        agent_analytics, collect_usage, collect_usage_from_tasks, render_agent_analytics,
        render_usage, UsageWindow,
    };
    use crate::config::AidConfig;
    use crate::store::Store;
    use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
    use chrono::{Duration, Local};

    fn make_task(id: &str, agent: AgentKind, tokens: i64, cost_usd: f64) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
            resolved_prompt: None,
            status: TaskStatus::Done,
            parent_task_id: None,
            workgroup_id: None,
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
            duration_ms: Some(1000),
            model: None,
            cost_usd: Some(cost_usd),
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            verify_status: VerifyStatus::Skipped,
            read_only: false,
            budget: false,
        }
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

        let snapshot = collect_usage_from_tasks(
            &[done_task, failed_task, pending_retry],
            &AidConfig::default(),
        )
        .unwrap();
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
}
