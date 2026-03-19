// aid — Multi-AI CLI team orchestrator.
// Entry point wiring: modules, CLI parsing, and dispatch startup.

#[macro_use]
pub mod output;
mod agent;
mod background;
mod batch;
mod board;
mod cli_actions;
mod cmd;
mod cmd_dispatch;
mod commit;
mod config;
mod context;
mod cost;
mod explore;
mod hooks;
mod input_signal;
mod notify;
mod paths;
mod prompt;
mod process_guard;
mod pty_bridge;
mod pty_runner;
mod pty_watch;
mod rate_limit;
pub(crate) mod sanitize;
mod sandbox;
mod session;
mod shared_dir;
mod skills;
mod store;
mod store_workgroups;
mod project;
mod team;
mod templates;
#[cfg(test)]
mod test_subprocess;
mod compaction;
pub mod claudemd;
mod tui;
mod system_resources;
mod types;
mod update_check;
mod usage;
pub mod usage_report;
mod verify;
mod watcher;
mod webhook;
#[cfg(feature = "web")]
mod web;
mod workgroup;
mod worktree;
mod cli;

use crate::cli::Cli;
use anyhow::Result;
use clap::Parser;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    output::init();
    if cli.quiet {
        output::set_quiet(true);
    }

    paths::ensure_dirs()?;
    let config = config::load_config().unwrap_or_default();
    if config.updates.check {
        update_check::maybe_check_update();
    }
    let store = Arc::new(store::Store::open(&paths::db_path())?);
    let _ = background::check_zombie_tasks(&store);

    cmd_dispatch::dispatch(store, cli.command).await
}
