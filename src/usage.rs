// Usage and cost snapshots built from task history plus config budgets.
// Exports collect_usage() and render_usage() for the `aid usage` command.

use anyhow::Result;
use chrono::{DateTime, Duration, Local};
use serde::Serialize;

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
    Ok(UsageSnapshot {
        generated_at: Local::now(),
        agent_rows: collect_agent_rows(tasks),
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

fn collect_agent_rows(tasks: &[Task]) -> Vec<AgentUsageRow> {
    let mut rows = Vec::new();
    for agent in [
        AgentKind::Codex,
        AgentKind::Gemini,
        AgentKind::OpenCode,
        AgentKind::Cursor,
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
                .map(|name| task.agent.as_str() == name)
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
    use super::{collect_usage, collect_usage_from_tasks, render_usage};
    use crate::config::AidConfig;
    use crate::store::Store;
    use crate::types::{AgentKind, Task, TaskId, TaskStatus};
    use chrono::Local;

    fn make_task(id: &str, agent: AgentKind, tokens: i64, cost_usd: f64) -> Task {
        Task {
            id: TaskId(id.to_string()),
            agent,
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
            duration_ms: Some(1000),
            model: None,
            cost_usd: Some(cost_usd),
            created_at: Local::now(),
            completed_at: None,
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
}
