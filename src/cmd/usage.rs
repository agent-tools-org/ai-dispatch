// Handler for `aid usage` — show tracked task usage and configured budgets.
// Combines SQLite task history with ~/.aid/config.toml budget entries.

use anyhow::Result;
use chrono::Local;
use serde_json::to_string;
use std::sync::Arc;

use crate::config;
use crate::session;
use crate::store::Store;
use crate::types::TaskFilter;
use crate::usage;

pub fn run(store: &Arc<Store>, session: bool, agent: Option<String>, period: String, json: bool) -> Result<()> {
    let config = config::load_config()?;
    let tasks = if session {
        let Some(caller) = session::current_caller() else {
            eprintln!("[aid] No active session detected");
            return Ok(());
        };
        store.list_tasks_by_session(&caller.session_id)?
    } else {
        store.list_tasks(TaskFilter::All)?
    };
    let window = usage::UsageWindow::parse(&period)?;
    let now = Local::now();
    if let Some(agent_name) = agent {
        let analytics = usage::agent_analytics(&tasks, &agent_name, window, now);
        if json {
            let report = usage::UsageReport::Agent { analytics };
            println!("{}", to_string(&report)?);
        } else {
            print!("{}", usage::render_agent_analytics(&analytics));
        }
    } else {
        let snapshot = usage::collect_usage_snapshot(&tasks, &config, window, now)?;
        if json {
            let report = usage::UsageReport::Summary { window, snapshot };
            println!("{}", to_string(&report)?);
        } else {
            print!("{}", usage::render_usage(&snapshot));
        }
    }
    Ok(())
}
