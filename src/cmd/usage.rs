// Handler for `aid usage` — show tracked task usage and configured budgets.
// Combines SQLite task history with ~/.aid/config.toml budget entries.

use anyhow::Result;
use chrono::Local;
use std::sync::Arc;

use crate::config;
use crate::runtime::session;
use crate::store::Store;
use crate::config::team;
use crate::types::TaskFilter;
use crate::cost::usage;
use crate::cost::usage_report;

#[path = "usage_billing.rs"]
mod billing;

pub fn run(
    store: &Arc<Store>,
    session: bool,
    agent: Option<String>,
    team_filter: Option<String>,
    period: String,
    json: bool,
) -> Result<()> {
    let config = config::load_config()?;
    let mut tasks = if session {
        let Some(caller) = session::current_caller() else {
            aid_info!("[aid] No active session detected");
            return Ok(());
        };
        store.list_tasks_by_session(&caller.session_id)?
    } else {
        store.list_tasks(TaskFilter::All)?
    };
    filter_team(&mut tasks, team_filter.as_deref());
    let window = usage::UsageWindow::parse(&period)?;
    let now = Local::now();
    let overage = billing::collect(store, &tasks, agent.as_deref(), window, now)?;
    if let Some(agent_name) = agent {
        let analytics = usage::agent_analytics(&tasks, &agent_name, window, now);
        if json {
            let report = usage::UsageReport::Agent { analytics };
            println!("{}", billing::json(report, &overage)?);
        } else {
            print!("{}", usage_report::render_agent_analytics(&analytics));
        }
    } else {
        let snapshot = usage::collect_usage_snapshot(&tasks, &config, window, now)?;
        if json {
            let report = usage::UsageReport::Summary { window, snapshot };
            println!("{}", billing::json(report, &overage)?);
        } else {
            print!("{}", usage_report::render_usage(&snapshot));
        }
    }
    if !json {
        print!("{}", billing::render(&overage));
    }
    Ok(())
}

fn filter_team(tasks: &mut Vec<crate::types::Task>, team_filter: Option<&str>) {
    // Filter tasks to team members if --team is set
    if let Some(team_name) = team_filter {
        if let Some(tc) = team::resolve_team(team_name) {
            let members: Vec<String> = tc
                .preferred_agents
                .iter()
                .map(|a| a.to_lowercase())
                .collect();
            tasks.retain(|t| {
                members
                    .iter()
                    .any(|m| t.agent_display_name().eq_ignore_ascii_case(m))
            });
            aid_info!(
                "[aid] Filtering usage to team '{}' ({} preferred agents)",
                team_name,
                tc.preferred_agents.len()
            );
        } else {
            aid_warn!("[aid] Warning: team '{team_name}' not found, showing all usage");
        }
    }
}
