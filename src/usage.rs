// Usage data structures and budget analytics for the `aid usage` command.
// Exports collection helpers and agent analytics without formatting.

use anyhow::Result;
use chrono::{DateTime, Duration, Local, LocalResult, TimeZone};
use serde::Serialize;

use crate::config::{AidConfig, UsageBudget};
use crate::store::Store;
use crate::types::{Task, TaskFilter, TaskStatus};
use crate::usage_report::{collect_agent_rows, collect_budget_rows, select_top_tasks};

pub struct BudgetStatus {
    pub over_limit: bool,
    pub near_limit: bool,
    pub message: Option<String>,
}

#[derive(Serialize)]
pub struct UsageSnapshot {
    pub(crate) generated_at: DateTime<Local>,
    pub(crate) agent_rows: Vec<AgentUsageRow>,
    pub(crate) budget_rows: Vec<BudgetUsageRow>,
}

#[derive(Default, Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum UsageWindow {
    #[default]
    All,
    Today,
    Days(u32),
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
pub(crate) struct AgentUsageRow {
    pub(crate) name: &'static str,
    pub(crate) tasks: usize,
    pub(crate) tokens: i64,
    pub(crate) cost_usd: f64,
    pub(crate) success_rate: f64,
    pub(crate) avg_duration_secs: f64,
    pub(crate) retry_count: usize,
    pub(crate) last_task_at: Option<DateTime<Local>>,
}

#[derive(Serialize)]
pub(crate) struct BudgetUsageRow {
    pub(crate) name: String,
    pub(crate) plan: Option<String>,
    pub(crate) window: Option<String>,
    pub(crate) tasks: u32,
    pub(crate) task_limit: Option<u32>,
    pub(crate) tokens: i64,
    pub(crate) token_limit: Option<i64>,
    pub(crate) cost_usd: f64,
    pub(crate) cost_limit_usd: Option<f64>,
    pub(crate) requests: u32,
    pub(crate) request_limit: Option<u32>,
    pub(crate) resets_at: Option<String>,
    pub(crate) notes: Option<String>,
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
    let now = Local::now();
    let mut over_limit = false;
    let mut near_limit = false;
    let mut messages: Vec<String> = Vec::new();

    for budget in &config.usage.budgets {
        let since = budget
            .window
            .as_deref()
            .and_then(parse_window)
            .map(|window| now - window);
        let (task_count, total_tokens, total_cost) = match budget.agent.as_deref() {
            Some(agent) => store.budget_usage_summary(agent, since)?,
            None => (0, 0, 0.0),
        };
        let tasks = task_count + budget.external_tasks;
        let _tokens = total_tokens + budget.external_tokens;
        let cost_usd = total_cost + budget.external_cost_usd;

        if let Some(limit) = budget.task_limit {
            if tasks >= limit {
                over_limit = true;
                messages.push(format!(
                    "Budget '{}': task limit reached ({}/{})",
                    budget.name, tasks, limit
                ));
            } else if tasks as f64 / limit as f64 > 0.8 {
                near_limit = true;
                messages.push(format!(
                    "Budget '{}': task usage at {}%",
                    budget.name,
                    (tasks as f64 / limit as f64 * 100.0) as u32
                ));
            }
        }
        if let Some(limit) = budget.cost_limit_usd {
            if cost_usd >= limit {
                over_limit = true;
                messages.push(format!(
                    "Budget '{}': cost limit reached (${:.2}/${:.2})",
                    budget.name, cost_usd, limit
                ));
            } else if cost_usd / limit > 0.8 {
                near_limit = true;
                messages.push(format!(
                    "Budget '{}': cost usage at {}%",
                    budget.name,
                    (cost_usd / limit * 100.0) as u32
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
    let trend = window.previous_range(now).map(|range| {
        let previous_tasks = filter_tasks_in_range(&matching_tasks, Some(range));
        let previous_stats = summarize_agent_period(&previous_tasks);
        AgentTrend {
            label: window.previous_label(),
            current: AgentTrendStats {
                tasks: stats.tasks,
                cost_usd: stats.cost_usd,
            },
            previous: AgentTrendStats {
                tasks: previous_stats.tasks,
                cost_usd: previous_stats.cost_usd,
            },
        }
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

pub(crate) fn filter_budget_tasks<'a>(tasks: &'a [Task], budget: &UsageBudget) -> Vec<&'a Task> {
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

pub(crate) fn parse_window(value: &str) -> Option<Duration> {
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
    let Some(naive) = date.and_hms_opt(0, 0, 0) else { return now; };
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(dt) => dt,
        LocalResult::Ambiguous(dt, _) => dt,
        LocalResult::None => Local.from_utc_datetime(&naive),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        agent_analytics, collect_usage, collect_usage_from_tasks, UsageWindow,
    };
    use crate::config::AidConfig;
    use crate::store::Store;
    use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
    use chrono::{Duration, Local};

    use crate::usage_report::{render_agent_analytics, render_usage};

    fn make_task(id: &str, agent: AgentKind, tokens: i64, cost_usd: f64) -> Task {
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
            worktree_path: None,
            worktree_branch: None,
            start_sha: None,
            log_path: None,
            output_path: None,
            tokens: Some(tokens),
            prompt_tokens: None,
            duration_ms: Some(1000),
            model: None,
            cost_usd: Some(cost_usd),
            exit_code: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            verify_status: VerifyStatus::Skipped,
            pending_reason: None,
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
