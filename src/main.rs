// aid — Multi-AI CLI team orchestrator.
// Entry point wiring: modules, CLI parsing, and dispatch startup.

#![allow(dead_code)]
#![allow(clippy::boxed_local)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::collapsible_else_if)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::explicit_auto_deref)]
#![allow(clippy::filter_map_bool_then)]
#![allow(clippy::manual_clamp)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::manual_unwrap_or)]
#![allow(clippy::needless_return)]
#![allow(clippy::should_implement_trait)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]
#![allow(clippy::unnecessary_map_or)]

#[macro_use]
pub mod output;
mod agent;
mod agent_config;
mod aic;
mod background;
mod batch;
mod board;
mod cli_actions;
mod cmd;
mod cmd_dispatch;
mod commit;
mod config;
mod container;
mod context;
mod cost;
pub mod credential_pool;
mod explore;
pub mod gitbutler;
mod hooks;
mod idle_timeout;
mod input_signal;
mod notify;
mod paths;
mod prompt;
mod prompt_scan;
mod process_guard;
mod pty_bridge;
mod pty_runner;
mod pty_watch;
mod rate_limit;
mod repo_root;
pub(crate) mod sanitize;
mod sandbox;
mod session;
mod shared_dir;
mod skills;
mod state;
#[cfg(test)]
mod state_tests;
mod store;
mod store_workgroups;
mod project;
mod team;
mod templates;
mod toolbox;
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
mod worktree_gc;
mod worktree_deps;
mod worktree;
mod cli;

use crate::cli::{Cli, Commands};
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

    match cli.command {
        Some(command) => cmd_dispatch::dispatch(store, command).await,
        None => cmd_dispatch::dispatch(store, Commands::Board(Default::default())).await,
    }
}
