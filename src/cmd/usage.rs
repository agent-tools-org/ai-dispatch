// Handler for `aid usage` — show tracked task usage and configured budgets.
// Combines SQLite task history with ~/.aid/config.toml budget entries.

use anyhow::Result;
use std::sync::Arc;

use crate::config;
use crate::session;
use crate::store::Store;
use crate::usage;

pub fn run(store: &Arc<Store>, session: bool) -> Result<()> {
    let config = config::load_config()?;
    let snapshot = if session {
        let Some(caller) = session::current_caller() else {
            eprintln!("[aid] No active session detected");
            return Ok(());
        };
        let tasks = store.list_tasks_by_session(&caller.session_id)?;
        usage::collect_usage_from_tasks(&tasks, &config)?
    } else {
        usage::collect_usage(store, &config)?
    };
    print!("{}", usage::render_usage(&snapshot));
    Ok(())
}
