// Agent performance dashboard for the `aid stats` command.
// Exports: run(). Deps: crate::store::Store, crate::types, crate::usage, crate::cost.

use anyhow::Result;
use chrono::{DateTime, Local};
use std::collections::{BTreeMap, HashMap};

use crate::cost;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskStatus};
use crate::usage::UsageWindow;

#[derive(Debug, PartialEq)] struct StatsSnapshot { agent_rows: Vec<AgentRow>, failure_rows: Vec<FailureRow>, model_rows: Vec<ModelRow> }
#[derive(Debug, PartialEq)] struct AgentRow { agent: String, tasks: usize, success_rate: f64, avg_duration_ms: Option<i64>, cost: String }
#[derive(Debug, PartialEq)] struct FailureRow { label: String, tasks: usize, agents: Vec<(String, usize)> }
#[derive(Debug, PartialEq)] struct ModelRow { model: String, tasks: usize, cost: String }

pub fn run(store: &Store, window: String, agent: Option<String>) -> Result<()> {
    let window = UsageWindow::parse(&window)?;
    let stats = collect(store, window, agent.as_deref(), Local::now())?;
    print!("{}", render_output(&stats, window));
    Ok(())
}

fn collect(store: &Store, window: UsageWindow, agent: Option<&str>, now: DateTime<Local>) -> Result<StatsSnapshot> {
    let tasks: Vec<Task> = store.list_tasks(crate::types::TaskFilter::All)?.into_iter().filter(|t| matches!(window.range(now), Some((s, e)) if t.created_at >= s && t.created_at <= e) || matches!(window, UsageWindow::All)).filter(|t| agent.is_none_or(|name| t.agent_display_name().eq_ignore_ascii_case(name))).collect();
    let mut agents: BTreeMap<String, (AgentKind, usize, usize, usize, i64, usize, f64)> = BTreeMap::new();
    let mut failures: HashMap<String, (usize, BTreeMap<String, usize>)> = HashMap::new();
    let mut models: BTreeMap<String, (usize, f64, AgentKind)> = BTreeMap::new();
    for task in &tasks {
        let row = agents.entry(task.agent_display_name().to_string()).or_insert((task.agent, 0, 0, 0, 0, 0, 0.0));
        row.1 += 1;
        row.2 += usize::from(matches!(task.status, TaskStatus::Done | TaskStatus::Merged));
        row.3 += usize::from(task.status != TaskStatus::Waiting);
        if let Some(ms) = task.duration_ms { row.4 += ms; row.5 += 1; }
        row.6 += task_cost(task);
        let model = task.model.clone().unwrap_or_else(|| "unknown".to_string());
        let model_row = models.entry(model).or_insert((0, 0.0, task.agent));
        model_row.0 += 1;
        model_row.1 += task_cost(task);
        if task.status == TaskStatus::Failed {
            let label = classify_failure(store.latest_error(task.id.as_str()).as_deref(), task.exit_code);
            let entry = failures.entry(label).or_insert((0, BTreeMap::new()));
            entry.0 += 1;
            *entry.1.entry(task.agent_display_name().to_string()).or_default() += 1;
        }
    }
    let mut agent_rows: Vec<_> = agents.into_iter().map(|(agent, (kind, tasks, success, success_base, duration_ms, duration_count, cost_usd))| AgentRow {
        agent, tasks, success_rate: if success_base == 0 { 0.0 } else { success as f64 * 100.0 / success_base as f64 }, avg_duration_ms: (duration_count > 0).then(|| duration_ms / duration_count as i64), cost: cost::format_cost_label(Some(cost_usd), kind),
    }).collect();
    agent_rows.sort_by(|a, b| b.tasks.cmp(&a.tasks).then_with(|| a.agent.cmp(&b.agent)));
    let mut failure_rows: Vec<_> = failures.into_iter().map(|(label, (tasks, agents))| {
        let mut agents: Vec<_> = agents.into_iter().collect();
        agents.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        FailureRow { label, tasks, agents }
    }).collect();
    failure_rows.sort_by(|a, b| b.tasks.cmp(&a.tasks).then_with(|| a.label.cmp(&b.label)));
    failure_rows.truncate(5);
    let mut model_rows: Vec<_> = models.into_iter().map(|(model, (tasks, cost_usd, kind))| ModelRow {
        model, tasks, cost: cost::format_cost_label(Some(cost_usd), kind),
    }).collect();
    model_rows.sort_by(|a, b| b.tasks.cmp(&a.tasks).then_with(|| a.model.cmp(&b.model)));
    Ok(StatsSnapshot { agent_rows, failure_rows, model_rows })
}

fn render_output(stats: &StatsSnapshot, window: UsageWindow) -> String {
    if stats.agent_rows.is_empty() {
        return format!("No tasks matched the selected filters for {}.\n", window.description());
    }
    render(stats, window)
}

fn render(stats: &StatsSnapshot, window: UsageWindow) -> String {
    let mut out = format!("Agent Performance ({})\n", window.description());
    for row in &stats.agent_rows { out.push_str(&format!("  {:<10} {:>3} tasks  {:>3.0}% success  avg {:<7}  {}\n", row.agent, row.tasks, row.success_rate, format_duration(row.avg_duration_ms), row.cost)); }
    if stats.agent_rows.is_empty() { out.push_str("  (none)\n"); }
    out.push_str("\nTop Failure Causes\n");
    for (index, row) in stats.failure_rows.iter().enumerate() {
        let breakdown = row.agents.iter().map(|(agent, count)| format!("{agent}: {count}")).collect::<Vec<_>>().join(", ");
        out.push_str(&format!("  {}. {:<28} {:>3} tasks  ({})\n", index + 1, row.label, row.tasks, breakdown));
    }
    if stats.failure_rows.is_empty() { out.push_str("  (none)\n"); }
    out.push_str("\nModel Usage\n");
    for row in &stats.model_rows { out.push_str(&format!("  {:<18} {:>3} tasks  {}\n", row.model, row.tasks, row.cost)); }
    if stats.model_rows.is_empty() { out.push_str("  (none)\n"); }
    out
}

fn classify_failure(detail: Option<&str>, exit_code: Option<i32>) -> String {
    let text = detail.unwrap_or("unknown failure");
    let lower = text.to_ascii_lowercase();
    if lower.contains("verify failed") { "verify failed".to_string() } else if lower.contains("hung: no output") { "agent hung: no output".to_string() } else if lower.contains("usage limit") || lower.contains("quota") { "usage limit".to_string() } else if let Some(idx) = lower.find("exit code ") {
        let suffix = &text[idx..];
        suffix.split_whitespace().take(3).collect::<Vec<_>>().join(" ")
    } else { exit_code.map(|code| format!("exit code {code}")).unwrap_or_else(|| text.to_string()) }
}

fn format_duration(duration_ms: Option<i64>) -> String {
    let secs = duration_ms.unwrap_or(0) / 1_000;
    match (secs / 60, secs % 60) { (0, s) => format!("{s}s"), (m, 0) => format!("{m}m"), (m, s) => format!("{m}m {s}s") }
}

fn task_cost(task: &Task) -> f64 {
    task.cost_usd.unwrap_or_else(|| {
        if task.agent == AgentKind::Cursor { 0.0 } else { cost::estimate_cost(task.tokens.unwrap_or(0), task.model.as_deref(), task.agent).unwrap_or(0.0) }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use crate::types::{EventKind, TaskEvent, TaskId, VerifyStatus};

    fn task(id: &str, agent: AgentKind, status: TaskStatus, age_days: i64, model: &str, cost_usd: Option<f64>, duration_ms: Option<i64>) -> Task {
        Task { id: TaskId(id.to_string()), agent, custom_agent_name: None, prompt: "prompt".to_string(), resolved_prompt: None, category: None, status, parent_task_id: None, workgroup_id: None, caller_kind: None, caller_session_id: None, agent_session_id: None, repo_path: None, worktree_path: None, worktree_branch: None, start_sha: None, log_path: None, output_path: None, tokens: Some(1_000), prompt_tokens: None, duration_ms, model: Some(model.to_string()), cost_usd, exit_code: None, created_at: Local::now() - Duration::days(age_days), completed_at: None, verify: None, verify_status: VerifyStatus::Skipped, pending_reason: None, read_only: false, budget: false }
    }

    #[test]
    fn collects_agent_failure_and_model_stats() {
        let store = Store::open_memory().unwrap();
        let now = Local::now();
        for task in [task("t-1", AgentKind::Codex, TaskStatus::Done, 1, "gpt-5.4", Some(10.0), Some(120_000)), task("t-2", AgentKind::Codex, TaskStatus::Failed, 2, "gpt-5.4", Some(5.0), Some(60_000)), task("t-3", AgentKind::Cursor, TaskStatus::Merged, 1, "composer-2", None, Some(90_000)), task("t-4", AgentKind::OpenCode, TaskStatus::Failed, 8, "glm-4.7", Some(1.0), Some(30_000))] {
            store.insert_task(&task).unwrap();
        }
        store.insert_event(&TaskEvent { task_id: TaskId("t-2".to_string()), timestamp: now, event_kind: EventKind::Error, detail: "verify failed (cargo check)".to_string(), metadata: None }).unwrap();
        let stats = collect(&store, UsageWindow::Days(7), None, now).unwrap();
        assert_eq!(stats.agent_rows[0], AgentRow { agent: "codex".to_string(), tasks: 2, success_rate: 50.0, avg_duration_ms: Some(90_000), cost: "$15.00".to_string() });
        assert_eq!(stats.agent_rows[1], AgentRow { agent: "cursor".to_string(), tasks: 1, success_rate: 100.0, avg_duration_ms: Some(90_000), cost: "subscription".to_string() });
        assert_eq!(stats.failure_rows, vec![FailureRow { label: "verify failed".to_string(), tasks: 1, agents: vec![("codex".to_string(), 1)] }]);
        assert_eq!(stats.model_rows[0], ModelRow { model: "gpt-5.4".to_string(), tasks: 2, cost: "$15.00".to_string() });
    }

    #[test]
    fn stats_does_not_panic_on_zero_duration_count() {
        let store = Store::open_memory().unwrap();
        let task = task("t-no-dur", AgentKind::Codex, TaskStatus::Done, 1, "gpt-5.4", Some(1.0), None);
        store.insert_task(&task).unwrap();

        let stats = collect(&store, UsageWindow::Days(7), None, Local::now()).unwrap();

        assert_eq!(stats.agent_rows[0].avg_duration_ms, None);
    }

    #[test]
    fn render_output_shows_friendly_message_when_no_tasks_match() {
        let stats = StatsSnapshot { agent_rows: Vec::new(), failure_rows: Vec::new(), model_rows: Vec::new() };

        assert_eq!(render_output(&stats, UsageWindow::Days(7)), "No tasks matched the selected filters for last 7 days.\n");
    }
}
