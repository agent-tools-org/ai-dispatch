// Project state CLI rendering for `aid project state`.
// Exports: run plus local formatting helpers and tests.
// Deps: crate::{paths, project, state, store}, anyhow, chrono.

use super::current_git_root;
use crate::project;
use crate::state::{self, ProjectState};
use crate::store::Store;
use anyhow::Result;
use chrono::{DateTime, Local};
use std::path::Path;

type AgentRate = (String, f64, usize);

pub(super) fn run() -> Result<()> {
    let git_root = current_git_root()?;
    let repo_path = git_root.to_string_lossy().into_owned();
    let store = Store::open(&crate::paths::db_path())?;
    let state = state::compute_state(&store, &repo_path)?;
    state::save_state(&state)?;
    let agent_rates = store.project_agent_success_rates(&repo_path)?;
    println!("{}", render_state(&git_root, &state, &agent_rates));
    Ok(())
}

fn render_state(git_root: &Path, state: &ProjectState, agent_rates: &[AgentRate]) -> String {
    let recent_total = usize::min(state.health.total_tasks as usize, 50);
    let recent_successes = (state.health.recent_success_rate * recent_total as f64).round() as usize;
    let project_name = project_name(git_root);
    let branch = state.context.active_branch.as_deref().unwrap_or("-");
    let last_task = match (&state.context.last_task_id, &state.context.last_task_agent) {
        (Some(id), Some(agent)) => format!("{id} ({agent})"),
        (Some(id), None) => id.clone(),
        _ => "-".to_string(),
    };
    let avg_cost = state
        .performance
        .avg_task_cost_usd
        .map(|value| format!("${value:.2}/task"))
        .unwrap_or_else(|| "-".to_string());

    [
        format!("Project State: {project_name}"),
        format!("  Updated: {}", format_timestamp(&state.last_updated)),
        String::new(),
        "Health:".to_string(),
        format!(
            "  Success rate: {:.0}% ({recent_successes}/{recent_total} recent tasks)",
            state.health.recent_success_rate * 100.0
        ),
        format!("  Last verify: {}", last_verify_label(state)),
        String::new(),
        "Performance:".to_string(),
        format!("  Best agent: {}", best_agent_label(state, agent_rates)),
        format!("  Avg cost: {avg_cost}"),
        String::new(),
        "Context:".to_string(),
        format!("  Branch: {branch}"),
        format!("  Last task: {last_task}"),
    ]
    .join("\n")
}

fn project_name(git_root: &Path) -> String {
    project::detect_project()
        .map(|config| config.id)
        .or_else(|| {
            git_root
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn best_agent_label(state: &ProjectState, agent_rates: &[AgentRate]) -> String {
    match &state.performance.best_agent {
        Some(best_agent) => agent_rates
            .iter()
            .find(|(agent, _, _)| agent == best_agent)
            .map(|(agent, rate, count)| format!("{agent} ({:.0}%, {count} tasks)", rate * 100.0))
            .unwrap_or_else(|| best_agent.clone()),
        None => "n/a".to_string(),
    }
}

fn last_verify_label(state: &ProjectState) -> String {
    match (
        state.health.last_verify_status.as_deref(),
        state.health.last_verify_time.as_deref(),
    ) {
        (Some(status), Some(time)) => format!("{status} ({})", format_relative_time(time)),
        (Some(status), None) => status.to_string(),
        _ => "unknown".to_string(),
    }
}

fn format_timestamp(value: &str) -> String {
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|_| value.to_string())
}

fn format_relative_time(value: &str) -> String {
    let Ok(parsed) = DateTime::parse_from_rfc3339(value) else {
        return value.to_string();
    };
    let seconds = Local::now()
        .signed_duration_since(parsed.with_timezone(&Local))
        .num_seconds();
    if seconds <= 0 {
        "just now".to_string()
    } else if seconds < 60 {
        format!("{seconds}s ago")
    } else if seconds < 3600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h ago", seconds / 3600)
    } else {
        format!("{}d ago", seconds / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::{render_state, AgentRate};
    use crate::state::{ContextState, HealthState, LearnedState, PerformanceState, ProjectState};
    use chrono::{Duration, Local};
    use std::collections::BTreeMap;
    use std::path::Path;

    #[test]
    fn render_state_includes_requested_sections() {
        let state = ProjectState {
            last_updated: Local::now().to_rfc3339(),
            health: HealthState {
                last_verify_status: Some("passed".to_string()),
                last_verify_time: Some((Local::now() - Duration::hours(2)).to_rfc3339()),
                recent_success_rate: 0.94,
                total_tasks: 50,
            },
            performance: PerformanceState {
                best_agent: Some("codex".to_string()),
                agent_success_rates: BTreeMap::from([("codex".to_string(), 0.92)]),
                avg_task_duration_secs: None,
                avg_task_cost_usd: Some(2.15),
            },
            context: ContextState {
                last_task_id: Some("t-abcd".to_string()),
                last_task_agent: Some("codex".to_string()),
                active_branch: Some("main".to_string()),
            },
            learned: LearnedState {
                effective_tools: Vec::new(),
                common_failure_patterns: Vec::new(),
            },
        };
        let agent_rates: Vec<AgentRate> = vec![("codex".to_string(), 0.92, 45)];

        let rendered = render_state(Path::new("/tmp/ai-dispatch"), &state, &agent_rates);

        assert!(rendered.contains("Project State: ai-dispatch"));
        assert!(rendered.contains("Success rate: 94% (47/50 recent tasks)"));
        assert!(rendered.contains("Last verify: passed (2h ago)"));
        assert!(rendered.contains("Best agent: codex (92%, 45 tasks)"));
        assert!(rendered.contains("Avg cost: $2.15/task"));
        assert!(rendered.contains("Branch: main"));
        assert!(rendered.contains("Last task: t-abcd (codex)"));
    }
}
