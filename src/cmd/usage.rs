// Handler for `aid usage` — show tracked task usage and configured budgets.
// Combines SQLite task history with ~/.aid/config.toml budget entries.

use anyhow::Result;
use std::sync::Arc;

use crate::config;
use crate::store::Store;
use crate::usage;

pub fn run(store: &Arc<Store>) -> Result<()> {
    let config = config::load_config()?;
    let snapshot = usage::collect_usage(store, &config)?;
    print!("{}", usage::render_usage(&snapshot));
    Ok(())
}
