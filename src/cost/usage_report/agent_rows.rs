// Groups usage by the persisted dispatch name, including API account routes.
// Computes execution statistics without combining unrelated named accounts.
use crate::cost::{self, usage::AgentUsageRow};
use crate::types::Task;

pub(crate) fn collect_agent_rows(tasks: &[Task]) -> Vec<AgentUsageRow> {
    let mut rows = Vec::new();
    let names: std::collections::BTreeSet<_> = tasks.iter().map(|task| task.agent_display_name()).collect();
    for name in names {
        let agent_tasks: Vec<&Task> = tasks
            .iter()
            .filter(|task| task.agent_display_name() == name)
            .collect();
        if agent_tasks.is_empty() {
            continue;
        }

        rows.push(summarize(name, &agent_tasks));
    }
    rows
}

fn summarize(name: &str, agent_tasks: &[&Task]) -> AgentUsageRow {
    let done_count = agent_tasks
        .iter()
        .filter(|task| task.outcome().is_success())
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
    let measured_count = agent_tasks
        .iter()
        .filter(|task| task.outcome() != crate::types::TaskOutcome::Stopped)
        .count();
    let success_rate = if measured_count == 0 {
        0.0
    } else {
        (done_count as f64 * 100.0) / measured_count as f64
    };
    let avg_duration_secs = if completed_durations.is_empty() {
        0.0
    } else {
        completed_durations.iter().sum::<i64>() as f64 / completed_durations.len() as f64 / 1000.0
    };
    let tokens = agent_tasks.iter().filter_map(|task| task.tokens).sum();
    let cost_usd = agent_tasks.iter().filter_map(|task| task.cost_usd).sum();
    let unknown_cost_tasks = cost::unknown_cost_tasks(agent_tasks.iter().copied());
    let last_task_at = agent_tasks.iter().map(|task| task.created_at).max();
    AgentUsageRow {
        name: name.to_string(),
        tasks: agent_tasks.len(),
        tokens,
        cost_usd,
        unknown_cost_tasks,
        success_rate,
        avg_duration_secs,
        retry_count,
        last_task_at,
    }
}
