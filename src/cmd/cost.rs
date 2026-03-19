// Handler for `aid cost` rollups by workgroup, day, and agent.
// Exports run() plus small aggregation helpers for tests.

use anyhow::{Result, bail};
use chrono::{DateTime, Local, NaiveDate};
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::cost;
use crate::store::Store;
use crate::types::{Task, TaskFilter};
use crate::usage::UsageWindow;

type Totals = (usize, i64, f64);

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DailyCostRow {
    pub(crate) date: NaiveDate,
    pub(crate) tasks: usize,
    pub(crate) tokens: i64,
    pub(crate) cost_usd: f64,
}

pub fn run(
    store: &Arc<Store>,
    group: Option<String>,
    summary: bool,
    agent: Option<String>,
    period: String,
) -> Result<()> {
    let mode_count = usize::from(group.is_some()) + usize::from(summary) + usize::from(agent.is_some());
    if mode_count != 1 {
        bail!("Select exactly one of --group, --summary, or --agent");
    }
    let tasks = store.list_tasks(TaskFilter::All)?;
    let window = UsageWindow::parse(&period)?;
    let output = match (group, agent) {
        (Some(group_id), None) => render_task_report(
            &format!("Workgroup cost for {group_id}"),
            &group_tasks(&tasks, &group_id),
        ),
        (None, Some(agent_name)) => render_task_report(
            &format!("Agent cost for {agent_name} ({})", window.description()),
            &agent_tasks(&tasks, &agent_name, window, Local::now()),
        ),
        (None, None) if summary => render_daily_report(&daily_summary_rows(&tasks, window, Local::now())),
        _ => unreachable!(),
    };
    print!("{output}");
    Ok(())
}

pub(crate) fn group_tasks<'a>(tasks: &'a [Task], group_id: &str) -> Vec<&'a Task> {
    tasks
        .iter()
        .filter(|task| task.workgroup_id.as_deref() == Some(group_id))
        .collect()
}

pub(crate) fn agent_tasks<'a>(
    tasks: &'a [Task],
    agent_name: &str,
    window: UsageWindow,
    now: DateTime<Local>,
) -> Vec<&'a Task> {
    tasks
        .iter()
        .filter(|task| task.agent_display_name().eq_ignore_ascii_case(agent_name))
        .filter(|task| in_window(task, window, now))
        .collect()
}

pub(crate) fn daily_summary_rows(
    tasks: &[Task],
    window: UsageWindow,
    now: DateTime<Local>,
) -> (Vec<DailyCostRow>, Totals) {
    let mut rows: BTreeMap<NaiveDate, (usize, i64, f64)> = BTreeMap::new();
    let mut totals = (0, 0, 0.0);
    for task in tasks.iter().filter(|task| in_window(task, window, now)) {
        let cost_usd = task_cost(task);
        let entry = rows.entry(task.created_at.date_naive()).or_insert((0, 0, 0.0));
        entry.0 += 1;
        entry.1 += task.tokens.unwrap_or(0);
        entry.2 += cost_usd;
        totals.0 += 1;
        totals.1 += task.tokens.unwrap_or(0);
        totals.2 += cost_usd;
    }
    let rows = rows
        .into_iter()
        .rev()
        .map(|(date, (tasks, tokens, cost_usd))| DailyCostRow {
            date,
            tasks,
            tokens,
            cost_usd,
        })
        .collect();
    (rows, totals)
}

fn render_task_report(title: &str, tasks: &[&Task]) -> String {
    let mut out = format!("{title}\n{:<10} {:<12} {:<8} {:<10} {:<10} {}\n", "Task ID", "Agent", "Status", "Tokens", "Cost", "Duration");
    out.push_str(&"-".repeat(68));
    out.push('\n');
    let mut totals = (0, 0, 0.0);
    for task in tasks {
        let tokens = task.tokens.unwrap_or(0);
        let cost_usd = task_cost(task);
        totals.0 += 1;
        totals.1 += tokens;
        totals.2 += cost_usd;
        out.push_str(&format!(
            "{:<10} {:<12} {:<8} {:<10} {:<10} {}\n",
            task.id,
            task.agent_display_name(),
            task.status.label(),
            tokens,
            cost::format_cost(Some(cost_usd)),
            format_duration(task.duration_ms),
        ));
    }
    out.push_str(&format!(
        "Total: {} tasks | {} tokens | {}\n",
        totals.0,
        totals.1,
        cost::format_cost(Some(totals.2))
    ));
    out
}

fn render_daily_report((rows, totals): &(Vec<DailyCostRow>, Totals)) -> String {
    let mut out = format!("Cost summary\n{:<12} {:<8} {:<10} {:<10}\n", "Date", "Tasks", "Tokens", "Cost");
    out.push_str(&"-".repeat(44));
    out.push('\n');
    for row in rows {
        out.push_str(&format!(
            "{:<12} {:<8} {:<10} {:<10}\n",
            row.date,
            row.tasks,
            row.tokens,
            cost::format_cost(Some(row.cost_usd)),
        ));
    }
    out.push_str(&format!("Total: {} tasks | {} tokens | {}\n", totals.0, totals.1, cost::format_cost(Some(totals.2))));
    out
}

fn in_window(task: &Task, window: UsageWindow, now: DateTime<Local>) -> bool {
    match window.range(now) {
        Some((start, end)) => task.created_at >= start && task.created_at <= end,
        None => true,
    }
}

fn format_duration(duration_ms: Option<i64>) -> String {
    match duration_ms {
        Some(ms) => format!("{:.1}s", ms as f64 / 1000.0),
        None => "-".to_string(),
    }
}

fn task_cost(task: &Task) -> f64 {
    task.cost_usd
        .or_else(|| cost::estimate_cost(task.tokens.unwrap_or(0), task.model.as_deref(), task.agent))
        .unwrap_or(0.0)
}
